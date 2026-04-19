//! Svelte linter. Runs lint rules on a parsed `SvelteAst`.

pub mod rules;

use oxc::span::Span;
use crate::ast::*;

/// A lint diagnostic.
#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub rule_name: &'static str,
    pub message: String,
    pub span: Span,
    pub fix: Option<Fix>,
}

/// An auto-fix: replace a span of source text.
#[derive(Debug, Clone)]
pub struct Fix {
    pub span: Span,
    pub replacement: String,
}

/// Rule configuration loaded from config files.
#[derive(Debug, Clone, Default)]
pub struct RuleConfig {
    /// Raw JSON options from the config file
    pub options: Option<serde_json::Value>,
    /// Parsed settings
    pub settings: Option<serde_json::Value>,
}

/// Context provided to lint rules during execution.
pub struct LintContext<'a> {
    pub ast: &'a SvelteAst,
    pub source: &'a str,
    pub config: RuleConfig,
    /// Path to the file being linted (for cross-file resolution)
    pub file_path: Option<String>,
    /// True when linting a `.svelte.js` or `.svelte.ts` module (not a `.svelte` component).
    pub is_svelte_module: bool,
    /// Pre-parsed JS/TS semantic model for the instance `<script>` block.
    /// `None` if there is no instance script or it failed to parse.
    pub instance_semantic: Option<&'a oxc::semantic::Semantic<'a>>,
    /// Pre-parsed JS/TS semantic model for the module (`<script module>`) block.
    pub module_semantic: Option<&'a oxc::semantic::Semantic<'a>>,
    /// Byte offset in `source` where the instance script's content starts
    /// (i.e. just after the `<script ...>` open tag's `>`). 0 if no instance.
    pub instance_content_offset: u32,
    /// Byte offset in `source` where the module script's content starts.
    pub module_content_offset: u32,
    diagnostics: Vec<LintDiagnostic>,
    current_rule: &'static str,
}

impl<'a> LintContext<'a> {
    pub fn new(ast: &'a SvelteAst, source: &'a str) -> Self {
        Self {
            ast, source, config: RuleConfig::default(), file_path: None, is_svelte_module: false,
            instance_semantic: None, module_semantic: None,
            instance_content_offset: 0, module_content_offset: 0,
            diagnostics: Vec::new(), current_rule: "",
        }
    }

    pub fn with_config(ast: &'a SvelteAst, source: &'a str, config: RuleConfig) -> Self {
        Self {
            ast, source, config, file_path: None, is_svelte_module: false,
            instance_semantic: None, module_semantic: None,
            instance_content_offset: 0, module_content_offset: 0,
            diagnostics: Vec::new(), current_rule: "",
        }
    }

    /// Semantic model for the "primary" script in the current lint mode.
    /// For a normal component lint, returns the instance script's semantic.
    /// For a `.svelte.js/.ts` module lint (`is_svelte_module`), returns the instance
    /// script's semantic (we stash the file's content in `instance` for module lints).
    pub fn primary_semantic(&self) -> Option<&'a oxc::semantic::Semantic<'a>> {
        self.instance_semantic
    }

    /// Content offset paired with `primary_semantic`.
    pub fn primary_content_offset(&self) -> u32 {
        self.instance_content_offset
    }

    #[inline]
    pub fn diagnostic(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(LintDiagnostic {
            rule_name: self.current_rule, message: message.into(), span, fix: None,
        });
    }

    pub fn diagnostic_with_fix(&mut self, message: impl Into<String>, span: Span, fix: Fix) {
        self.diagnostics.push(LintDiagnostic {
            rule_name: self.current_rule, message: message.into(), span, fix: Some(fix),
        });
    }

    #[inline]
    fn set_rule(&mut self, name: &'static str) { self.current_rule = name; }
    pub fn into_diagnostics(self) -> Vec<LintDiagnostic> { self.diagnostics }
}

/// The trait all lint rules implement.
pub trait Rule: Send + Sync {
    fn name(&self) -> &'static str;
    fn is_recommended(&self) -> bool { false }
    fn is_fixable(&self) -> bool { false }
    /// Whether this rule can apply to plain .js/.ts files (not just .svelte).
    fn applies_to_scripts(&self) -> bool { false }
    /// Whether this rule can apply to .svelte.js/.svelte.ts modules.
    fn applies_to_svelte_scripts(&self) -> bool { false }
    fn run(&self, ctx: &mut LintContext);
}

/// The linter: holds rules and runs them on parsed files.
pub struct Linter {
    rules: Vec<Box<dyn Rule>>,
    has_script_rules: bool,
}

impl Linter {
    pub fn recommended() -> Self {
        let rules = rules::recommended_rules();
        let has_script_rules = rules.iter().any(|r| r.applies_to_scripts());
        Self { rules, has_script_rules }
    }
    pub fn all() -> Self {
        let rules = rules::all_rules();
        let has_script_rules = rules.iter().any(|r| r.applies_to_scripts());
        Self { rules, has_script_rules }
    }

    pub fn with_custom_rules(mut self, rules: Vec<Box<dyn Rule>>) -> Self {
        self.has_script_rules =
            self.has_script_rules || rules.iter().any(|r| r.applies_to_scripts());
        self.rules.extend(rules);
        self
    }

    /// Remove rules that are set to "off" in the config.
    pub fn remove_disabled_rules(&mut self, config: &crate::config::OxvelteConfig) {
        self.rules.retain(|r| !config.is_rule_off(r.name()));
        self.has_script_rules = self.rules.iter().any(|r| r.applies_to_scripts());
    }

    pub fn rules(&self) -> &[Box<dyn Rule>] {
        &self.rules
    }

    pub fn lint(&self, ast: &SvelteAst, source: &str) -> Vec<LintDiagnostic> {
        self.lint_impl(ast, source, RuleConfig::default(), None, ScriptMode::Full, /*is_svelte_module*/ false)
    }

    /// Lint a plain JS/TS file. Only runs rules marked with `applies_to_scripts`.
    /// Wraps the source in a synthetic SvelteAst with the content as an instance script.
    pub fn lint_script(&self, source: &str) -> Vec<LintDiagnostic> {
        if !self.has_script_rules { return vec![]; }
        use crate::ast::{SvelteAst, Script, Fragment};
        let ast = SvelteAst {
            html: Fragment { nodes: vec![], span: oxc::span::Span::new(0, 0) },
            instance: Some(Script {
                content: source.to_string(),
                module: false,
                lang: None,
                strict_events: false,
                span: oxc::span::Span::new(0, source.len() as u32),
            }),
            module: None,
            css: None,
        };
        self.lint_impl(&ast, source, RuleConfig::default(), None, ScriptMode::ScriptOnly, /*is_svelte_module*/ false)
    }

    /// Lint a `.svelte.js` or `.svelte.ts` module. Runs rules marked with
    /// `applies_to_scripts` or `applies_to_svelte_scripts`.
    pub fn lint_svelte_script(&self, source: &str, is_ts: bool) -> Vec<LintDiagnostic> {
        let ast = SvelteAst {
            html: Fragment { nodes: vec![], span: oxc::span::Span::new(0, 0) },
            instance: Some(Script {
                content: source.to_string(),
                module: false,
                lang: if is_ts { Some("ts".to_string()) } else { None },
                strict_events: false,
                span: oxc::span::Span::new(0, source.len() as u32),
            }),
            module: None,
            css: None,
        };
        self.lint_impl(&ast, source, RuleConfig::default(), None, ScriptMode::SvelteModule, /*is_svelte_module*/ true)
    }

    pub fn lint_with_config(&self, ast: &SvelteAst, source: &str, config: RuleConfig) -> Vec<LintDiagnostic> {
        self.lint_impl(ast, source, config, None, ScriptMode::Full, false)
    }

    pub fn lint_with_config_and_path(&self, ast: &SvelteAst, source: &str, config: RuleConfig, file_path: &str) -> Vec<LintDiagnostic> {
        self.lint_impl(ast, source, config, Some(file_path.to_string()), ScriptMode::Full, false)
    }

    /// Central lint driver. Creates an allocator, parses script blocks into oxc ASTs
    /// (with semantic), populates `LintContext`, runs each applicable rule, and filters
    /// suppressed diagnostics.
    fn lint_impl(
        &self,
        ast: &SvelteAst,
        source: &str,
        config: RuleConfig,
        file_path: Option<String>,
        script_mode: ScriptMode,
        is_svelte_module: bool,
    ) -> Vec<LintDiagnostic> {
        use oxc::allocator::Allocator;
        use oxc::parser::Parser;
        use oxc::semantic::SemanticBuilder;
        use oxc::span::SourceType;

        let alloc = Allocator::default();

        // Compute content offsets first (don't depend on parse success).
        let instance_offset = ast.instance.as_ref().map(|s| {
            if is_svelte_module { s.span.start } else { script_content_offset(s, source) }
        }).unwrap_or(0);
        let module_offset = ast.module.as_ref().map(|s| script_content_offset(s, source)).unwrap_or(0);

        // Parse each script. On parse error we just leave semantic as None — rules that
        // need a semantic model early-return, matching existing behavior.
        let instance_parse = ast.instance.as_ref().and_then(|s| {
            if s.content.trim().is_empty() { return None; }
            let st = if matches!(s.lang.as_deref(), Some("ts" | "typescript")) { SourceType::ts() } else { SourceType::mjs() };
            let r = Parser::new(&alloc, &s.content, st).parse();
            if !r.errors.is_empty() { None } else { Some(r) }
        });
        let module_parse = ast.module.as_ref().and_then(|s| {
            if s.content.trim().is_empty() { return None; }
            let st = if matches!(s.lang.as_deref(), Some("ts" | "typescript")) { SourceType::ts() } else { SourceType::mjs() };
            let r = Parser::new(&alloc, &s.content, st).parse();
            if !r.errors.is_empty() { None } else { Some(r) }
        });
        let instance_semantic = instance_parse.as_ref().map(|p| SemanticBuilder::new().build(&p.program).semantic);
        let module_semantic = module_parse.as_ref().map(|p| SemanticBuilder::new().build(&p.program).semantic);

        let mut ctx = LintContext::with_config(ast, source, config);
        ctx.file_path = file_path;
        ctx.is_svelte_module = is_svelte_module;
        ctx.instance_semantic = instance_semantic.as_ref();
        ctx.module_semantic = module_semantic.as_ref();
        ctx.instance_content_offset = instance_offset;
        ctx.module_content_offset = module_offset;

        for rule in &self.rules {
            let include = match script_mode {
                ScriptMode::Full => true,
                ScriptMode::ScriptOnly => rule.applies_to_scripts(),
                ScriptMode::SvelteModule => rule.applies_to_scripts() || rule.applies_to_svelte_scripts(),
            };
            if !include { continue; }
            ctx.set_rule(rule.name());
            rule.run(&mut ctx);
        }
        filter_suppressed(ctx.into_diagnostics(), source)
    }
}

/// Which subset of rules to run, based on the kind of file being linted.
#[derive(Clone, Copy)]
enum ScriptMode {
    /// Normal `.svelte` component — run every rule.
    Full,
    /// Plain `.js`/`.ts` — run only `applies_to_scripts` rules.
    ScriptOnly,
    /// `.svelte.js`/`.svelte.ts` module — run `applies_to_scripts` or `applies_to_svelte_scripts`.
    SvelteModule,
}

/// Byte offset in the original source where a `<script ...>` block's content starts
/// (i.e. just after the open tag's `>`).
fn script_content_offset(script: &Script, source: &str) -> u32 {
    let tag_text = &source[script.span.start as usize..script.span.end as usize];
    let gt = tag_text.find('>').unwrap_or(0);
    script.span.start + gt as u32 + 1
}

// ---------------------------------------------------------------------------
// Ignore-comment support
// ---------------------------------------------------------------------------
//
// Recognizes these comment directives (in JS/TS comments and HTML comments):
//
//   File-level disable (until re-enabled or EOF):
//     /* eslint-disable */                    — all rules
//     /* eslint-disable rule1, rule2 */       — specific rules
//     /* oxlint-disable rule1 */
//     /* oxvelte-disable rule1 */
//     /* eslint-enable */                     — re-enable
//
//   Next-line disable:
//     // eslint-disable-next-line             — all rules
//     // eslint-disable-next-line rule1       — specific rules
//     // oxlint-disable-next-line rule1
//     // oxvelte-disable-next-line rule1
//     <!-- svelte-ignore rule1 rule2 -->      — template (next sibling)
//     <!-- eslint-disable-next-line rule1 --> — template variant
//     <!-- oxvelte-disable-next-line rule1 -->
//
//   Same-line disable:
//     code // eslint-disable-line rule1
//     code // oxlint-disable-line rule1
//     code // oxvelte-disable-line rule1

/// A parsed ignore directive.
#[derive(Debug)]
enum Directive {
    /// Disable rules from this line until re-enabled.
    DisableBlock { line: usize, rules: Vec<String> },
    /// Re-enable rules.
    EnableBlock { line: usize, rules: Vec<String> },
    /// Disable rules on the next line only.
    DisableNextLine { line: usize, rules: Vec<String> },
    /// Disable rules on this line only.
    DisableLine { line: usize, rules: Vec<String> },
}

/// Parse all ignore directives from source text.
fn parse_directives(source: &str) -> Vec<Directive> {
    let mut directives = Vec::new();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        // --- HTML comments: <!-- svelte-ignore ... --> or <!-- eslint-disable-next-line ... -->
        if let Some(inner) = extract_html_comment(trimmed) {
            let inner = inner.trim();

            // <!-- svelte-ignore rule1 rule2 (optional notes) -->
            if let Some(rest) = inner.strip_prefix("svelte-ignore") {
                let rules = parse_svelte_ignore_rules(rest.trim());
                if !rules.is_empty() {
                    directives.push(Directive::DisableNextLine { line: line_idx, rules });
                }
                continue;
            }

            // <!-- eslint-disable-next-line rule1, rule2 -->
            // <!-- oxlint-disable-next-line rule1, rule2 -->
            // <!-- oxvelte-disable-next-line rule1, rule2 -->
            for prefix in &["eslint-disable-next-line", "oxlint-disable-next-line", "oxvelte-disable-next-line"] {
                if let Some(rest) = inner.strip_prefix(prefix) {
                    let rules = parse_rule_list(rest.trim());
                    directives.push(Directive::DisableNextLine { line: line_idx, rules });
                }
            }

            // <!-- eslint-disable --> / <!-- eslint-enable -->
            // <!-- eslint-disable rule1, rule2 --> / <!-- eslint-enable rule1, rule2 -->
            for prefix_base in &["eslint", "oxlint", "oxvelte"] {
                let disable = format!("{}-disable", prefix_base);
                let enable = format!("{}-enable", prefix_base);

                if let Some(rest) = inner.strip_prefix(disable.as_str()) {
                    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
                        if !rest.trim_start().starts_with("next-line") && !rest.trim_start().starts_with("line") {
                            let rules = parse_rule_list(rest.trim());
                            directives.push(Directive::DisableBlock { line: line_idx, rules });
                        }
                    }
                }
                if let Some(rest) = inner.strip_prefix(enable.as_str()) {
                    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
                        let rules = parse_rule_list(rest.trim());
                        directives.push(Directive::EnableBlock { line: line_idx, rules });
                    }
                }
            }
            continue;
        }

        // --- JS line comments: // eslint-disable-next-line ...
        if let Some(comment) = extract_js_line_comment(line) {
            let comment = comment.trim();

            // // svelte-ignore rule1 rule2
            if let Some(rest) = comment.strip_prefix("svelte-ignore") {
                let rules = parse_svelte_ignore_rules(rest.trim());
                if !rules.is_empty() {
                    directives.push(Directive::DisableNextLine { line: line_idx, rules });
                }
                continue;
            }

            for prefix_base in &["eslint", "oxlint", "oxvelte"] {
                let dnl = format!("{}-disable-next-line", prefix_base);
                let dl = format!("{}-disable-line", prefix_base);

                if let Some(rest) = comment.strip_prefix(dnl.as_str()) {
                    let rules = parse_rule_list(rest.trim());
                    directives.push(Directive::DisableNextLine { line: line_idx, rules });
                } else if let Some(rest) = comment.strip_prefix(dl.as_str()) {
                    let rules = parse_rule_list(rest.trim());
                    directives.push(Directive::DisableLine { line: line_idx, rules });
                }
            }
        }

        // --- JS block comments: /* eslint-disable */ or /* eslint-enable */
        if let Some(comment) = extract_js_block_comment(trimmed) {
            let comment = comment.trim();
            for prefix_base in &["eslint", "oxlint", "oxvelte"] {
                let disable = format!("{}-disable", prefix_base);
                let enable = format!("{}-enable", prefix_base);

                // Must match "eslint-disable" but NOT "eslint-disable-next-line" or "eslint-disable-line"
                if let Some(rest) = comment.strip_prefix(disable.as_str()) {
                    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
                        if !rest.trim_start().starts_with("next-line") && !rest.trim_start().starts_with("line") {
                            let rules = parse_rule_list(rest.trim());
                            directives.push(Directive::DisableBlock { line: line_idx, rules });
                        }
                    }
                } else if let Some(rest) = comment.strip_prefix(enable.as_str()) {
                    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
                        let rules = parse_rule_list(rest.trim());
                        directives.push(Directive::EnableBlock { line: line_idx, rules });
                    }
                }
            }
        }
    }

    directives
}

/// Extract content from `<!-- ... -->`.
fn extract_html_comment(s: &str) -> Option<&str> {
    let s = s.strip_prefix("<!--")?;
    let s = s.strip_suffix("-->")?;
    Some(s)
}

/// Extract content from `// ...` (the first `//` in the line).
fn extract_js_line_comment(line: &str) -> Option<&str> {
    // Find // that's not inside a string — simplified: just find the first //
    let pos = line.find("//")?;
    Some(&line[pos + 2..])
}

/// Extract content from `/* ... */`.
fn extract_js_block_comment(s: &str) -> Option<&str> {
    let s = s.strip_prefix("/*")?;
    let s = s.strip_suffix("*/")?;
    Some(s)
}

/// Parse comma-separated rule names. Empty list means "all rules".
fn parse_rule_list(s: &str) -> Vec<String> {
    if s.is_empty() { return Vec::new(); } // empty = all rules
    s.split(|c: char| c == ',' || c == ' ')
        .map(|r| r.trim())
        .filter(|r| !r.is_empty() && !r.starts_with("--"))
        .map(|r| r.to_string())
        .collect()
}

/// Parse svelte-ignore rules (space or comma separated, may have parenthesized notes).
fn parse_svelte_ignore_rules(s: &str) -> Vec<String> {
    let mut rules = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let rest_trimmed = rest.trim_start();
        if rest_trimmed.is_empty() { break; }
        // Skip parenthesized notes: (reason text)
        if rest_trimmed.starts_with('(') {
            if let Some(close) = rest_trimmed.find(')') {
                rest = &rest_trimmed[close + 1..];
                continue;
            }
            break;
        }
        // Extract rule name (until space, comma, or paren)
        let end = rest_trimmed.find(|c: char| c == ' ' || c == ',' || c == '(')
            .unwrap_or(rest_trimmed.len());
        let rule = &rest_trimmed[..end];
        if !rule.is_empty() {
            // Normalize underscores to hyphens for svelte rules
            rules.push(rule.replace('_', "-"));
        }
        rest = &rest_trimmed[end..];
        // Skip comma
        rest = rest.trim_start_matches(',');
    }
    rules
}

/// Check if a rule name matches a directive's rule list.
/// If the directive's rule list is empty, it matches ALL rules.
fn rule_matches(rule_name: &str, directive_rules: &[String]) -> bool {
    if directive_rules.is_empty() { return true; } // empty = all rules
    directive_rules.iter().any(|r| {
        r == rule_name
            || rule_name.ends_with(r.as_str()) // "no-console" matches "svelte/no-console"
            || r.replace('_', "-") == rule_name.replace('_', "-") // normalize
    })
}

/// Filter diagnostics by removing any suppressed by ignore comments.
fn filter_suppressed(diagnostics: Vec<LintDiagnostic>, source: &str) -> Vec<LintDiagnostic> {
    if diagnostics.is_empty() { return diagnostics; }

    let directives = parse_directives(source);
    if directives.is_empty() { return diagnostics; }

    // Build line offset table for mapping span → line number
    let line_starts: Vec<usize> = std::iter::once(0)
        .chain(source.bytes().enumerate().filter(|(_, b)| *b == b'\n').map(|(i, _)| i + 1))
        .collect();

    let span_to_line = |offset: u32| -> usize {
        line_starts.partition_point(|&start| start <= offset as usize).saturating_sub(1)
    };

    diagnostics.into_iter().filter(|diag| {
        let diag_line = span_to_line(diag.span.start);

        for dir in &directives {
            match dir {
                Directive::DisableNextLine { line, rules } => {
                    if diag_line == line + 1 && rule_matches(diag.rule_name, rules) {
                        return false;
                    }
                }
                Directive::DisableLine { line, rules } => {
                    if diag_line == *line && rule_matches(diag.rule_name, rules) {
                        return false;
                    }
                }
                Directive::DisableBlock { line, rules } => {
                    if diag_line >= *line && rule_matches(diag.rule_name, rules) {
                        // Check if re-enabled before this diagnostic
                        let re_enabled = directives.iter().any(|d| {
                            if let Directive::EnableBlock { line: enable_line, rules: enable_rules } = d {
                                *enable_line > *line && *enable_line <= diag_line
                                    && (enable_rules.is_empty() || rules.iter().all(|r| enable_rules.contains(r)))
                            } else { false }
                        });
                        if !re_enabled { return false; }
                    }
                }
                Directive::EnableBlock { .. } => {} // handled inside DisableBlock
            }
        }
        true
    }).collect()
}

/// Walk all template nodes recursively, calling visitor on each.
pub fn walk_template_nodes<F>(fragment: &Fragment, visitor: &mut F)
where F: FnMut(&TemplateNode)
{
    walk_nodes(&fragment.nodes, visitor);
}

fn walk_nodes<F>(nodes: &[TemplateNode], visitor: &mut F)
where F: FnMut(&TemplateNode)
{
    for node in nodes {
        visitor(node);
        match node {
            TemplateNode::Element(el) => {
                walk_nodes(&el.children, visitor);
            }
            TemplateNode::IfBlock(block) => {
                walk_nodes(&block.consequent.nodes, visitor);
                if let Some(alt) = &block.alternate {
                    visitor(alt);
                    // Recurse into the alternate node
                    match alt.as_ref() {
                        TemplateNode::IfBlock(ib) => {
                            walk_nodes(&ib.consequent.nodes, visitor);
                            if let Some(a) = &ib.alternate { walk_alt(a, visitor); }
                        }
                        _ => {}
                    }
                }
            }
            TemplateNode::EachBlock(block) => {
                walk_nodes(&block.body.nodes, visitor);
                if let Some(fb) = &block.fallback { walk_nodes(&fb.nodes, visitor); }
            }
            TemplateNode::AwaitBlock(block) => {
                if let Some(p) = &block.pending { walk_nodes(&p.nodes, visitor); }
                if let Some(t) = &block.then { walk_nodes(&t.nodes, visitor); }
                if let Some(c) = &block.catch { walk_nodes(&c.nodes, visitor); }
            }
            TemplateNode::KeyBlock(block) => walk_nodes(&block.body.nodes, visitor),
            TemplateNode::SnippetBlock(block) => walk_nodes(&block.body.nodes, visitor),
            _ => {}
        }
    }
}

fn walk_alt<F>(alt: &Box<TemplateNode>, visitor: &mut F)
where F: FnMut(&TemplateNode)
{
    visitor(alt);
    if let TemplateNode::IfBlock(ib) = alt.as_ref() {
        walk_nodes(&ib.consequent.nodes, visitor);
        if let Some(a) = &ib.alternate { walk_alt(a, visitor); }
    }
}

/// Parse import statements from script content.
/// Returns a list of (local_name, imported_name, source_module) tuples.
pub fn parse_imports(content: &str) -> Vec<(String, String, String)> {
    let mut imports = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = content[search_from..].find("import ") {
        let abs = search_from + pos;
        // Make sure it's at a statement boundary
        if abs > 0 {
            let prev = content.as_bytes()[abs - 1];
            if prev.is_ascii_alphanumeric() || prev == b'_' {
                search_from = abs + 7;
                continue;
            }
        }
        let rest = &content[abs + 7..];
        // Find the "from" keyword and module string
        if let Some(from_pos) = rest.find("from ") {
            let specifier_text = &rest[..from_pos].trim();
            let module_text = &rest[from_pos + 5..];
            let module = extract_string_literal(module_text.trim());
            if let Some(module) = module {
                // Parse specifiers
                if specifier_text.starts_with('{') {
                    // Named imports: import { a, b as c } from 'mod'
                    let inner = specifier_text.trim_start_matches('{').trim_end_matches('}');
                    for spec in inner.split(',') {
                        let spec = spec.trim();
                        if spec.is_empty() { continue; }
                        if let Some(as_pos) = spec.find(" as ") {
                            let imported = spec[..as_pos].trim();
                            let local = spec[as_pos + 4..].trim();
                            imports.push((local.to_string(), imported.to_string(), module.clone()));
                        } else {
                            imports.push((spec.to_string(), spec.to_string(), module.clone()));
                        }
                    }
                } else if specifier_text.starts_with("* as ") {
                    // Namespace import: import * as name from 'mod'
                    let local = specifier_text[5..].trim();
                    imports.push((local.to_string(), "*".to_string(), module.clone()));
                } else {
                    // Default import: import name from 'mod'
                    let local = specifier_text.trim();
                    if !local.is_empty() {
                        imports.push((local.to_string(), "default".to_string(), module.clone()));
                    }
                }
            }
        }
        search_from = abs + 7;
    }
    imports
}

fn extract_string_literal(s: &str) -> Option<String> {
    if s.len() < 2 { return None; }
    let quote = s.as_bytes()[0];
    if quote != b'\'' && quote != b'"' && quote != b'`' { return None; }
    if let Some(end) = s[1..].find(quote as char) {
        return Some(s[1..1 + end].to_string());
    }
    None
}
