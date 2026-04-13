//! Custom rule support via embedded JavaScript engine (boa).
//!
//! Users write lint rules in JavaScript and reference them in `oxvelte.config.json`:
//!
//! ```json
//! { "customRules": ["./rules/*.js"] }
//! ```
//!
//! Rule file format:
//! ```javascript
//! export default {
//!   name: "custom/no-div-without-class",
//!   run(ctx) {
//!     ctx.walk((node) => {
//!       if (node.type === "Element" && node.name === "div") {
//!         const hasClass = node.attributes.some(a =>
//!           a.type === "NormalAttribute" && a.name === "class"
//!         );
//!         if (!hasClass) {
//!           ctx.diagnostic("div must have a class attribute", node.span);
//!         }
//!       }
//!     });
//!   }
//! };
//! ```

use crate::linter::{Fix, LintContext, Rule};
use boa_engine::{Context, Source};
use oxc::span::Span;
use std::path::{Path, PathBuf};

/// A lint rule implemented in JavaScript, executed via boa_engine.
pub struct ScriptRule {
    name: &'static str,
    source: String,
}

unsafe impl Send for ScriptRule {}
unsafe impl Sync for ScriptRule {}

impl Rule for ScriptRule {
    fn name(&self) -> &'static str {
        self.name
    }
    fn is_fixable(&self) -> bool {
        true
    }

    fn run(&self, ctx: &mut LintContext) {
        if let Err(e) = self.run_js(ctx) {
            eprintln!("oxvelte: error in custom rule {}: {}", self.name, e);
        }
    }
}

#[derive(serde::Deserialize)]
struct JsDiag {
    message: String,
    start: u32,
    end: u32,
    #[serde(rename = "fixStart")]
    fix_start: Option<u32>,
    #[serde(rename = "fixEnd")]
    fix_end: Option<u32>,
    #[serde(rename = "fixReplacement")]
    fix_replacement: Option<String>,
}

impl ScriptRule {
    fn run_js(&self, ctx: &mut LintContext) -> Result<(), String> {
        let mut js = Context::default();

        let ast_json =
            serde_json::to_string(&ctx.ast).map_err(|e| format!("AST serialize: {}", e))?;
        let source_json =
            serde_json::to_string(ctx.source).map_err(|e| format!("source serialize: {}", e))?;
        let options_json = ctx
            .config
            .options
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());
        let settings_json = ctx
            .config
            .settings
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "null".to_string());
        let file_path_json = ctx
            .file_path
            .as_ref()
            .map(|p| {
                format!(
                    "\"{}\"",
                    p.replace('\\', "\\\\").replace('"', "\\\"")
                )
            })
            .unwrap_or_else(|| "null".to_string());

        let script = format!(
            "{prelude}\n\
             var __ast = {ast};\n\
             var __source = {source};\n\
             var __options = {options};\n\
             var __settings = {settings};\n\
             var __filePath = {file_path};\n\
             var __diagnostics = [];\n\
             var __ctx = {{\n\
               ast: __ast,\n\
               source: __source,\n\
               filePath: __filePath,\n\
               options: __options,\n\
               settings: __settings,\n\
               walk: function(visitor) {{ __walk(__ast.html.nodes, visitor); }},\n\
               diagnostic: function(message, span) {{\n\
                 __diagnostics.push({{ message: String(message), start: span.start, end: span.end }});\n\
               }},\n\
               diagnosticWithFix: function(message, span, fix) {{\n\
                 __diagnostics.push({{\n\
                   message: String(message), start: span.start, end: span.end,\n\
                   fixStart: fix.span.start, fixEnd: fix.span.end,\n\
                   fixReplacement: String(fix.replacement)\n\
                 }});\n\
               }}\n\
             }};\n\
             {rule_code}\n\
             __rule.run(__ctx);\n\
             JSON.stringify(__diagnostics);",
            prelude = JS_PRELUDE,
            ast = ast_json,
            source = source_json,
            options = options_json,
            settings = settings_json,
            file_path = file_path_json,
            rule_code = self.source,
        );

        let result = js
            .eval(Source::from_bytes(script.as_bytes()))
            .map_err(|e| format!("{}", e))?;

        let json_str = result
            .to_string(&mut js)
            .map_err(|e| format!("{}", e))?
            .to_std_string_escaped();

        let diags: Vec<JsDiag> = serde_json::from_str(&json_str).unwrap_or_default();

        for d in diags {
            let span = Span::new(d.start, d.end);
            match (d.fix_start, d.fix_end, d.fix_replacement) {
                (Some(fs), Some(fe), Some(fr)) => {
                    ctx.diagnostic_with_fix(
                        d.message,
                        span,
                        Fix {
                            span: Span::new(fs, fe),
                            replacement: fr,
                        },
                    );
                }
                _ => ctx.diagnostic(d.message, span),
            }
        }

        Ok(())
    }
}

/// JS helper code injected before rule execution. Provides the `walk` function
/// that recursively visits all template nodes, matching Rust's `walk_template_nodes`.
const JS_PRELUDE: &str = r#"
function __walk(nodes, fn) {
  if (!Array.isArray(nodes)) return;
  for (var i = 0; i < nodes.length; i++) __visit(nodes[i], fn);
}
function __visit(n, fn) {
  if (!n || typeof n !== "object" || !n.type) return;
  fn(n);
  if (n.children) __walk(n.children, fn);
  if (n.consequent && n.consequent.nodes) __walk(n.consequent.nodes, fn);
  if (n.alternate) __visit(n.alternate, fn);
  if (n.body && n.body.nodes) __walk(n.body.nodes, fn);
  if (n.fallback && n.fallback.nodes) __walk(n.fallback.nodes, fn);
  if (n.pending && n.pending.nodes) __walk(n.pending.nodes, fn);
  if (n.then && n.then.nodes) __walk(n.then.nodes, fn);
  if (n.catch && n.catch.nodes) __walk(n.catch.nodes, fn);
}
"#;

/// Load custom rule files from glob patterns relative to `config_dir`.
pub fn load_custom_rules(patterns: &[String], config_dir: &Path) -> Vec<Box<dyn Rule>> {
    let paths = resolve_paths(patterns, config_dir);
    let mut rules: Vec<Box<dyn Rule>> = Vec::new();

    for path in paths {
        match load_rule_file(&path) {
            Ok(rule) => {
                eprintln!(
                    "oxvelte: loaded custom rule \"{}\" from {}",
                    rule.name,
                    path.display()
                );
                rules.push(Box::new(rule));
            }
            Err(e) => {
                eprintln!("oxvelte: error loading {}: {}", path.display(), e);
            }
        }
    }

    rules
}

fn load_rule_file(path: &Path) -> Result<ScriptRule, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("read error: {}", e))?;

    // Transform module syntax into a plain var assignment boa can eval.
    let transformed = if content.contains("export default") {
        content.replacen("export default", "var __rule =", 1)
    } else if content.contains("module.exports") {
        format!(
            "var __rule = (function() {{\
               var module = {{ exports: {{}} }};\
               var exports = module.exports;\
               {}\
               return module.exports;\
             }})()",
            content
        )
    } else {
        return Err(
            "Rule file must use `export default { ... }` or `module.exports = { ... }`".into(),
        );
    };

    let name = extract_rule_name(&transformed)?;
    let leaked: &'static str = Box::leak(name.into_boxed_str());

    Ok(ScriptRule {
        name: leaked,
        source: transformed,
    })
}

/// Evaluate the rule source just enough to read its `name` property.
fn extract_rule_name(source: &str) -> Result<String, String> {
    let mut ctx = Context::default();
    let script = format!("{}\nString(__rule.name)", source);
    let result = ctx
        .eval(Source::from_bytes(script.as_bytes()))
        .map_err(|e| format!("parse error: {}", e))?;
    let name = result
        .to_string(&mut ctx)
        .map_err(|e| format!("{}", e))?
        .to_std_string_escaped();

    if name.is_empty() || name == "undefined" {
        return Err("Rule must have a `name` property".into());
    }
    Ok(name)
}

/// Resolve glob patterns like `"./rules/*.js"` into concrete file paths.
fn resolve_paths(patterns: &[String], config_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for pattern in patterns {
        let full = config_dir.join(pattern);
        if full.is_file() {
            paths.push(full);
        } else if let Some(parent) = full.parent() {
            if parent.is_dir() {
                if let Some(name) = full.file_name().and_then(|n| n.to_str()) {
                    if let Some(ext) = name.strip_prefix("*.") {
                        if let Ok(entries) = std::fs::read_dir(parent) {
                            for entry in entries.flatten() {
                                let p = entry.path();
                                if p.is_file() && p.extension().is_some_and(|e| e == ext) {
                                    paths.push(p);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    paths.sort();
    paths
}
