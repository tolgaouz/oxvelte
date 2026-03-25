use oxvelte::{parser, linter};

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "oxvelte", version, about = "A fast Svelte linter powered by oxc")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the linter on Svelte files.
    Lint {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Run all rules instead of just recommended ones.
        #[arg(long)]
        all_rules: bool,
        /// Auto-fix problems (where supported).
        #[arg(long)]
        fix: bool,
        /// Output results as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Parse a Svelte file and dump the AST as JSON.
    Parse {
        file: PathBuf,
        /// Pretty-print the JSON output.
        #[arg(long, short)]
        pretty: bool,
        /// Output format: "legacy" (Svelte 4) or "modern" (Svelte 5).
        #[arg(long, default_value = "legacy")]
        format: String,
    },
    /// Parse + lint (alias for lint).
    Check {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// List all available lint rules.
    Rules,
    /// Migrate an ESLint svelte config to oxvelte.config.json.
    Migrate {
        /// Path to ESLint config file (.eslintrc.json, eslint.config.json, etc.)
        file: PathBuf,
        /// Write output to oxvelte.config.json instead of stdout.
        #[arg(long, short)]
        write: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Lint { paths, all_rules, fix: _, json } => cmd_lint(&paths, all_rules, json),
        Command::Parse { file, pretty, format } => cmd_parse(&file, pretty, &format),
        Command::Check { paths } => cmd_lint(&paths, false, false),
        Command::Rules => cmd_rules(),
        Command::Migrate { file, write } => cmd_convert_config(&file, write),
    }
}

struct FileDiagnostic {
    line: usize,
    col: usize,
    end_line: usize,
    end_col: usize,
    message: String,
    rule_name: &'static str,
}

struct FileResult {
    path: String,
    diagnostics: Vec<FileDiagnostic>,
}

fn cmd_lint(paths: &[PathBuf], all_rules: bool, json_output: bool) -> ExitCode {
    use rayon::prelude::*;

    let lint = if all_rules { linter::Linter::all() } else { linter::Linter::recommended() };
    let files = collect_lint_files(paths);
    let use_color = !json_output && atty::is(atty::Stream::Stderr);

    let mut file_results: Vec<FileResult> = files.par_iter().filter_map(|path| {
        let source = std::fs::read_to_string(path).ok()?;
        let is_svelte = path.extension().is_some_and(|e| e == "svelte");
        let diags = if is_svelte {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let result = parser::parse(&source);
                let file_path_str = path.to_string_lossy().to_string();
                lint.lint_with_config_and_path(&result.ast, &source, oxvelte::linter::RuleConfig::default(), &file_path_str)
            })) {
                Ok(d) => d,
                Err(_) => {
                    eprintln!("oxvelte: internal error parsing {}", path.display());
                    return None;
                }
            }
        } else {
            lint.lint_script(&source)
        };
        if diags.is_empty() { return None; }
        let abs_path = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let path_str = abs_path.display().to_string();
        let diagnostics: Vec<FileDiagnostic> = diags.iter().map(|d| {
            let (line, col) = offset_to_line_col(&source, d.span.start as usize);
            let (end_line, end_col) = offset_to_line_col(&source, d.span.end as usize);
            FileDiagnostic { line, col, end_line, end_col, message: d.message.clone(), rule_name: d.rule_name }
        }).collect();
        Some(FileResult { path: path_str, diagnostics })
    }).collect();

    file_results.sort_by(|a, b| a.path.cmp(&b.path));

    let total_diags: usize = file_results.iter().map(|f| f.diagnostics.len()).sum();
    let files_with_diags = file_results.len();

    if json_output {
        let json_results: Vec<serde_json::Value> = file_results.iter().flat_map(|f| {
            f.diagnostics.iter().map(|d| serde_json::json!({
                "file": &f.path,
                "rule": d.rule_name,
                "message": &d.message,
                "line": d.line,
                "column": d.col,
                "endLine": d.end_line,
                "endColumn": d.end_col,
            }))
        }).collect();
        println!("{}", serde_json::to_string_pretty(&json_results).unwrap_or_default());
    } else {
        for file_result in &file_results {
            // File header — underlined, clickable in VS Code
            if use_color {
                eprintln!("\n\x1b[4m{}\x1b[0m", file_result.path);
            } else {
                eprintln!("\n{}", file_result.path);
            }

            // Find max widths for alignment
            let max_loc_width = file_result.diagnostics.iter()
                .map(|d| format!("{}:{}", d.line, d.col).len())
                .max().unwrap_or(0);

            for d in &file_result.diagnostics {
                let loc = format!("{}:{}", d.line, d.col);
                if use_color {
                    eprintln!(
                        "  \x1b[2m{:<width$}\x1b[0m  \x1b[33mwarning\x1b[0m  {}  \x1b[2m{}\x1b[0m",
                        loc, d.message, d.rule_name, width = max_loc_width
                    );
                } else {
                    eprintln!(
                        "  {:<width$}  warning  {}  {}",
                        loc, d.message, d.rule_name, width = max_loc_width
                    );
                }
            }
        }

        if total_diags > 0 {
            if use_color {
                eprintln!("\n\x1b[1;31m\u{2716} {} problem(s) in {} file(s).\x1b[0m", total_diags, files_with_diags);
            } else {
                eprintln!("\n{} problem(s) in {} file(s).", total_diags, files_with_diags);
            }
        }
    }
    if total_diags > 0 { ExitCode::from(1) } else { ExitCode::SUCCESS }
}

fn cmd_parse(file: &PathBuf, pretty: bool, format: &str) -> ExitCode {
    use oxvelte::parser::serialize::{to_legacy_json, to_modern_json};

    let source = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => { eprintln!("Error reading {}: {}", file.display(), e); return ExitCode::from(1); }
    };
    let result = parser::parse(&source);
    for err in &result.errors {
        eprintln!("Parse error: {:?}", err);
    }

    let json_value = match format {
        "modern" => to_modern_json(&result.ast, &source),
        _ => to_legacy_json(&result.ast, &source),
    };

    let output = if pretty {
        serde_json::to_string_pretty(&json_value)
    } else {
        serde_json::to_string(&json_value)
    };
    match output {
        Ok(j) => { println!("{}", j); ExitCode::SUCCESS }
        Err(e) => { eprintln!("JSON error: {}", e); ExitCode::from(1) }
    }
}

fn cmd_rules() -> ExitCode {
    let rules = linter::rules::all_rules();
    let recommended = linter::rules::recommended_rules();
    let rec_names: std::collections::HashSet<String> = recommended.iter().map(|r| r.name().to_string()).collect();

    println!("{:<50} {:>5}  {:>5}", "Rule", "Rec", "Fix");
    println!("{}", "-".repeat(65));
    for rule in &rules {
        let name = rule.name();
        let is_rec = if rec_names.contains(name) { "  *  " } else { "     " };
        let is_fix = if rule.is_fixable() { "  *  " } else { "     " };
        println!("{:<50} {:>5}  {:>5}", name, is_rec, is_fix);
    }
    println!("\n{} rules total ({} recommended)", rules.len(), recommended.len());
    ExitCode::SUCCESS
}

fn cmd_convert_config(file: &PathBuf, write: bool) -> ExitCode {
    let content = match std::fs::read_to_string(file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading {}: {}", file.display(), e);
            return ExitCode::from(1);
        }
    };

    // Try to extract JSON from JS/MJS config files (common flat config pattern)
    let json_str = if file.extension().is_some_and(|e| e == "mjs" || e == "js" || e == "cjs") {
        eprintln!("Note: JS config files are partially supported. Only inline JSON objects with svelte rules will be extracted.");
        eprintln!("For best results, export your ESLint config as JSON first:");
        eprintln!("  npx eslint --print-config yourfile.svelte > eslint-resolved.json");
        eprintln!("  oxvelte convert-config eslint-resolved.json");
        // Try to extract rules from JS — look for "svelte/" patterns
        extract_rules_from_js(&content)
    } else {
        content.clone()
    };

    let config = match oxvelte::config::OxvelteConfig::from_eslint(&json_str) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error parsing config: {}", e);
            return ExitCode::from(1);
        }
    };

    if config.rules.is_empty() {
        eprintln!("No svelte/* rules found in the config.");
        return ExitCode::from(1);
    }

    let output = config.to_json();

    if write {
        match std::fs::write("oxvelte.config.json", &output) {
            Ok(_) => {
                eprintln!("Wrote oxvelte.config.json ({} rules)", config.rules.len());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error writing oxvelte.config.json: {}", e);
                ExitCode::from(1)
            }
        }
    } else {
        println!("{}", output);
        ExitCode::SUCCESS
    }
}

/// Best-effort extraction of svelte rules from a JS config file.
fn extract_rules_from_js(content: &str) -> String {
    let mut rules = serde_json::Map::new();
    // Find patterns like "svelte/rule-name": "error" or 'svelte/rule-name': ['error', {...}]
    for line in content.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        // Match: "svelte/rule-name": value  or  'svelte/rule-name': value
        for quote in &["\"", "'"] {
            let prefix = format!("{}svelte/", quote);
            if let Some(start) = trimmed.find(&prefix) {
                let after = &trimmed[start + 1..]; // skip opening quote
                if let Some(end) = after.find(*quote) {
                    let rule_name = &after[..end];
                    // Find the value after :
                    let rest = &after[end + 1..].trim_start();
                    if let Some(rest) = rest.strip_prefix(':') {
                        let val = rest.trim().trim_end_matches(',');
                        // Try to parse as JSON
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(val) {
                            rules.insert(format!("svelte/{}", rule_name.strip_prefix("svelte/").unwrap_or(rule_name)), v);
                        } else {
                            // Try with quotes normalized
                            let normalized = val.replace('\'', "\"");
                            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&normalized) {
                                rules.insert(format!("svelte/{}", rule_name.strip_prefix("svelte/").unwrap_or(rule_name)), v);
                            }
                        }
                    }
                }
            }
        }
    }
    let obj = serde_json::json!({ "rules": rules });
    serde_json::to_string(&obj).unwrap_or_default()
}

fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset { break; }
        if ch == '\n' { line += 1; col = 1; } else { col += 1; }
    }
    (line, col)
}

fn collect_lint_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let extensions = ["svelte", "js", "ts", "mjs", "mts"];
    for path in paths {
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    files.push(path.clone());
                }
            }
        } else if path.is_dir() {
            // Skip node_modules, .svelte-kit, build, dist
            let dirname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if matches!(dirname, "node_modules" | ".svelte-kit" | "build" | "dist" | ".git") {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(path) {
                let children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
                files.extend(collect_lint_files(&children));
            }
        }
    }
    files
}
