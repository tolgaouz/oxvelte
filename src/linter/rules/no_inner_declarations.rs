//! `svelte/no-inner-declarations` — disallow function declarations in nested blocks.
//! ⭐ Recommended (Extension Rule)
//!
//! Flags `function foo() {}` whose parent is not one of:
//! - `Program`        — top-level
//! - `FunctionBody`   — immediate body of a function
//! - `ExportNamedDeclaration` / `ExportDefaultDeclaration` — `export function foo() {}`

use crate::linter::{LintContext, Rule};
use oxc::ast::AstKind;
use oxc::span::Span;

pub struct NoInnerDeclarations;

impl Rule for NoInnerDeclarations {
    fn name(&self) -> &'static str {
        "svelte/no-inner-declarations"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;
        let nodes = semantic.nodes();

        for node in nodes.iter() {
            let AstKind::Function(f) = node.kind() else { continue };
            if !f.is_declaration() {
                continue;
            }
            // Direct parent must be Program / Export / StaticBlock / FunctionBody.
            let parent_kind = nodes.parent_kind(node.id());
            let direct_ok = matches!(
                parent_kind,
                AstKind::Program(_)
                    | AstKind::FunctionBody(_)
                    | AstKind::ExportNamedDeclaration(_)
                    | AstKind::ExportDefaultDeclaration(_)
                    | AstKind::StaticBlock(_)
            );
            if direct_ok {
                continue;
            }
            // Otherwise: the function is in a nested block. ESLint's
            // `no-inner-declarations` only flags this when the nearest enclosing
            // function/program is the MODULE (program). If there's an enclosing
            // function body, JS function-scope hoisting makes the inner
            // declaration fine.
            let mut cur = node.id();
            let mut in_function_scope = false;
            loop {
                let parent = nodes.parent_id(cur);
                if parent == cur {
                    break;
                }
                match nodes.kind(parent) {
                    AstKind::FunctionBody(_) => {
                        in_function_scope = true;
                        break;
                    }
                    AstKind::Program(_) => break,
                    _ => {}
                }
                cur = parent;
            }
            if in_function_scope {
                continue;
            }
            let span = f.id.as_ref().map_or(f.span, |id| id.span);
            let s = content_offset + span.start;
            let e = content_offset + span.end;
            ctx.diagnostic(
                "Move function declaration to program root.",
                Span::new(s, e),
            );
        }
    }
}
