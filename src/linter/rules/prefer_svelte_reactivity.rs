//! `svelte/prefer-svelte-reactivity` — prefer Svelte reactive classes over mutable
//! built-in JS classes (Date, Map, Set, URL, URLSearchParams).
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct PreferSvelteReactivity;

/// Built-in class info: (class_name, svelte_alternative, mutating_methods, mutating_properties)
struct BuiltinClass {
    name: &'static str,
    svelte_name: &'static str,
    /// Method names that mutate the instance (called as `var.method(...)`)
    mutating_methods: &'static [&'static str],
    /// Property names that can be assigned to (as `var.prop = ...`)
    mutating_props: &'static [&'static str],
}

const BUILTIN_CLASSES: &[BuiltinClass] = &[
    BuiltinClass {
        name: "Date",
        svelte_name: "SvelteDate",
        mutating_methods: &[
            "setDate", "setFullYear", "setHours", "setMilliseconds", "setMinutes",
            "setMonth", "setSeconds", "setTime", "setUTCDate", "setUTCFullYear",
            "setUTCHours", "setUTCMilliseconds", "setUTCMinutes", "setUTCMonth",
            "setUTCSeconds", "setYear",
        ],
        mutating_props: &[],
    },
    BuiltinClass {
        name: "Map",
        svelte_name: "SvelteMap",
        mutating_methods: &["set", "delete", "clear"],
        mutating_props: &[],
    },
    BuiltinClass {
        name: "Set",
        svelte_name: "SvelteSet",
        mutating_methods: &["add", "delete", "clear"],
        mutating_props: &[],
    },
    BuiltinClass {
        name: "URL",
        svelte_name: "SvelteURL",
        mutating_methods: &[],
        mutating_props: &[
            "hash", "host", "hostname", "href", "password",
            "pathname", "port", "protocol", "search", "username",
        ],
    },
    BuiltinClass {
        name: "URLSearchParams",
        svelte_name: "SvelteURLSearchParams",
        mutating_methods: &["append", "delete", "set", "sort"],
        mutating_props: &[],
    },
];

impl Rule for PreferSvelteReactivity {
    fn name(&self) -> &'static str {
        "svelte/prefer-svelte-reactivity"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let script = match &ctx.ast.instance {
            Some(s) => s,
            None => return,
        };

        let content = &script.content;
        let base = script.span.start as usize;

        // Collect imported/aliased names that shadow built-in classes
        let shadowed = collect_shadowed_classes(content);

        // Find `new ClassName(...)` patterns and check for mutations
        for builtin in BUILTIN_CLASSES {
            // Skip if this class name is imported from a package (shadowed)
            if shadowed.contains(&builtin.name.to_string()) {
                continue;
            }

            let new_pat = format!("new {}(", builtin.name);
            for (offset, _) in content.match_indices(&new_pat) {
                // Get the variable name this is assigned to
                let line_start = content[..offset].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = &content[line_start..content[offset..].find('\n').map(|p| offset + p).unwrap_or(content.len())];

                let var_name = extract_var_name(line, builtin.name);

                if let Some(var_name) = var_name {
                    // Check if the variable is mutated anywhere in the content
                    if is_mutated(content, &var_name, builtin) {
                        let new_keyword_pos = base + offset;
                        ctx.diagnostic(
                            format!(
                                "Found a mutable instance of the built-in {} class. Use {} instead.",
                                builtin.name, builtin.svelte_name
                            ),
                            Span::new(new_keyword_pos as u32, (new_keyword_pos + new_pat.len()) as u32),
                        );
                    }
                }
            }
        }
    }
}

/// Collect class names that are imported from packages (not built-in).
/// e.g., `import { Set } from "package"` shadows the built-in Set.
/// Also `import { SvelteDate as Date }` shadows Date.
fn collect_shadowed_classes(content: &str) -> Vec<String> {
    let mut shadowed = Vec::new();
    for line in content.lines() {
        let t = line.trim();
        if !t.starts_with("import ") { continue; }

        if let (Some(bs), Some(be)) = (t.find('{'), t.find('}')) {
            for imp in t[bs+1..be].split(',') {
                let imp = imp.trim();
                let local_name = if let Some(as_pos) = imp.find(" as ") {
                    imp[as_pos + 4..].trim()
                } else {
                    imp
                };

                // Check if this shadows a built-in class name
                for builtin in BUILTIN_CLASSES {
                    if local_name == builtin.name {
                        shadowed.push(builtin.name.to_string());
                    }
                }
            }
        }
    }
    shadowed
}

/// Extract variable name from a line like `const variable = new ClassName(...)`.
fn extract_var_name(line: &str, class_name: &str) -> Option<String> {
    let t = line.trim();
    // Pattern: `const/let/var NAME = new ClassName(`
    for kw in &["const ", "let ", "var "] {
        if let Some(rest) = t.strip_prefix(kw) {
            if let Some(eq_pos) = rest.find(" = ") {
                let name = rest[..eq_pos].trim();
                let after_eq = rest[eq_pos + 3..].trim();
                let new_pat = format!("new {}(", class_name);
                if after_eq.starts_with(&new_pat) {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}

/// Check if a variable is mutated using any of the built-in class's mutating methods or properties.
fn is_mutated(content: &str, var_name: &str, builtin: &BuiltinClass) -> bool {
    // Check mutating method calls: `var.method(`
    for method in builtin.mutating_methods {
        let pat = format!("{}.{}(", var_name, method);
        if content.contains(&pat) {
            return true;
        }
    }

    // Check mutating property assignments: `var.prop = ` (not `var.prop ==`)
    for prop in builtin.mutating_props {
        let pat = format!("{}.{} =", var_name, prop);
        if let Some(pos) = content.find(&pat) {
            // Make sure it's `= ` not `==`
            let after = &content[pos + pat.len()..];
            if !after.starts_with('=') {
                return true;
            }
        }
        // Also check `var.prop=` (no space before =)
        let pat2 = format!("{}.{}=", var_name, prop);
        if let Some(pos) = content.find(&pat2) {
            let after = &content[pos + pat2.len()..];
            if !after.starts_with('=') {
                return true;
            }
        }
    }

    false
}
