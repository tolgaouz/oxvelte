//! `svelte/no-inner-declarations` — disallow `function` (and optionally `var`) declarations
//! in nested blocks.
//!
//! ⭐ Recommended (Extension Rule)
//!
//! Mirrors ESLint core's `no-inner-declarations`:
//! - Default mode `"functions"` only flags `function` declarations.
//! - `"both"` also flags `var` declarations.
//!
//! The vendor rule (svelte/no-inner-declarations) wraps the core rule and
//! treats a `<script>` block as if its statements were directly under the
//! program root. We achieve the same by running over `ctx.instance_semantic`
//! and `ctx.module_semantic`, where the `Program` node IS the script body.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::VariableDeclarationKind;
use oxc::ast::AstKind;
use oxc::semantic::{AstNodes, NodeId};
use oxc::span::Span;

pub struct NoInnerDeclarations;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Functions,
    Both,
}

#[derive(Clone, Copy)]
enum Target {
    Program,
    FunctionBody,
    StaticBlock,
}

impl Target {
    fn body_phrase(self) -> &'static str {
        match self {
            Target::Program => "program",
            Target::FunctionBody => "function body",
            Target::StaticBlock => "class static block body",
        }
    }
}

/// Walk ancestors of `start` and return the nearest enclosing `Program`,
/// `FunctionBody`, or `StaticBlock`. Falls back to `Program` for safety.
fn nearest_target(start: NodeId, nodes: &AstNodes) -> Target {
    let mut cur = start;
    loop {
        let parent = nodes.parent_id(cur);
        if parent == cur {
            return Target::Program;
        }
        match nodes.kind(parent) {
            AstKind::Program(_) => return Target::Program,
            AstKind::FunctionBody(_) => return Target::FunctionBody,
            AstKind::StaticBlock(_) => return Target::StaticBlock,
            _ => {}
        }
        cur = parent;
    }
}

/// True when `kind` is one of the "this is already a scope root" placements
/// where neither a function nor a `var` declaration counts as nested.
fn is_root_scope_kind(kind: AstKind) -> bool {
    matches!(
        kind,
        AstKind::Program(_)
            | AstKind::FunctionBody(_)
            | AstKind::StaticBlock(_)
            | AstKind::ExportNamedDeclaration(_)
            | AstKind::ExportDefaultDeclaration(_)
    )
}

impl Rule for NoInnerDeclarations {
    fn name(&self) -> &'static str {
        "svelte/no-inner-declarations"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // Options: `[mode, { blockScopedFunctions }]`. Schema mirrors ESLint
        // core. We currently treat `blockScopedFunctions` as if it were always
        // `"disallow"` — the legacy behavior, which is what every fixture
        // exercises. Adding `"allow"` requires propagating strict-mode info
        // and is left for a follow-up.
        let mode = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .map(|s| if s == "both" { Mode::Both } else { Mode::Functions })
            .unwrap_or(Mode::Functions);

        // We collect (message, span) before reporting because `ctx.diagnostic`
        // borrows `ctx` mutably and we want to keep the semantic borrow live
        // through the iteration.
        let mut findings: Vec<(String, Span)> = Vec::new();

        for (semantic, content_offset) in [
            ctx.instance_semantic.map(|s| (s, ctx.instance_content_offset)),
            ctx.module_semantic.map(|s| (s, ctx.module_content_offset)),
        ]
        .into_iter()
        .flatten()
        {
            let nodes = semantic.nodes();
            for node in nodes.iter() {
                let parent_kind = nodes.parent_kind(node.id());
                match node.kind() {
                    AstKind::Function(f) if f.is_declaration() => {
                        if is_root_scope_kind(parent_kind) {
                            continue;
                        }
                        let target = nearest_target(node.id(), nodes);
                        let msg = format!(
                            "Move function declaration to {} root.",
                            target.body_phrase()
                        );
                        let s = content_offset + f.span.start;
                        let e = content_offset + f.span.end;
                        findings.push((msg, Span::new(s, e)));
                    }
                    AstKind::VariableDeclaration(vd)
                        if mode == Mode::Both && vd.kind == VariableDeclarationKind::Var =>
                    {
                        // `var` is allowed inside the init clause of a `for`,
                        // `for-in`, or `for-of` loop. Vendor doesn't flag those.
                        if is_root_scope_kind(parent_kind)
                            || matches!(
                                parent_kind,
                                AstKind::ForStatement(_)
                                    | AstKind::ForInStatement(_)
                                    | AstKind::ForOfStatement(_)
                            )
                        {
                            continue;
                        }
                        let target = nearest_target(node.id(), nodes);
                        let msg = format!(
                            "Move variable declaration to {} root.",
                            target.body_phrase()
                        );
                        let s = content_offset + vd.span.start;
                        let e = content_offset + vd.span.end;
                        findings.push((msg, Span::new(s, e)));
                    }
                    _ => {}
                }
            }
        }

        for (msg, span) in findings {
            ctx.diagnostic(msg, span);
        }
    }
}
