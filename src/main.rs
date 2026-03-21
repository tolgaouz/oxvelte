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
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Lint { paths, all_rules, fix: _, json } => cmd_lint(&paths, all_rules, json),
        Command::Parse { file, pretty, format } => cmd_parse(&file, pretty, &format),
        Command::Check { paths } => cmd_lint(&paths, false, false),
        Command::Rules => cmd_rules(),
    }
}

fn cmd_lint(paths: &[PathBuf], all_rules: bool, json_output: bool) -> ExitCode {
    let lint = if all_rules { linter::Linter::all() } else { linter::Linter::recommended() };
    let mut total_diags = 0u32;
    let mut total_files = 0u32;
    let mut json_results = Vec::new();

    for path in collect_svelte_files(paths) {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => { eprintln!("Error reading {}: {}", path.display(), e); continue; }
        };
        let result = parser::parse(&source);
        let diags = lint.lint(&result.ast, &source);
        total_files += 1;
        for d in &diags {
            total_diags += 1;
            let (line, col) = offset_to_line_col(&source, d.span.start as usize);
            let (end_line, end_col) = offset_to_line_col(&source, d.span.end as usize);
            if json_output {
                json_results.push(serde_json::json!({
                    "file": path.display().to_string(),
                    "rule": &d.rule_name,
                    "message": &d.message,
                    "line": line,
                    "column": col,
                    "endLine": end_line,
                    "endColumn": end_col,
                }));
            } else {
                eprintln!("{}:{}:{}: {} [{}]", path.display(), line, col, d.message, d.rule_name);
            }
        }
    }

    if json_output {
        println!("{}", serde_json::to_string_pretty(&json_results).unwrap_or_default());
    } else {
        eprintln!("\n{} problem(s) in {} file(s).", total_diags, total_files);
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

fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let mut line = 1;
    let mut col = 1;
    for (i, ch) in source.char_indices() {
        if i >= offset { break; }
        if ch == '\n' { line += 1; col = 1; } else { col += 1; }
    }
    (line, col)
}

fn collect_svelte_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_file() && path.extension().is_some_and(|e| e == "svelte") {
            files.push(path.clone());
        } else if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(path) {
                let children: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
                files.extend(collect_svelte_files(&children));
            }
        }
    }
    files
}
