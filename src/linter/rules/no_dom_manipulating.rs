//! `svelte/no-dom-manipulating` — disallow DOM manipulating.
//! ⭐ Recommended

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, AttributeValuePart, DirectiveKind, TemplateNode};
use oxc::ast::ast::{AssignmentTarget, Expression, MemberExpression, SimpleAssignmentTarget};
use oxc::ast::AstKind;
use oxc::semantic::SymbolId;
use oxc::span::{Ident, Span};
use rustc_hash::FxHashSet;

const DOM_METHODS: &[&str] = &[
    "appendChild", "removeChild", "insertBefore", "replaceChild",
    "normalize", "after", "append", "before",
    "insertAdjacentElement", "insertAdjacentHTML", "insertAdjacentText",
    "prepend", "remove", "replaceChildren", "replaceWith",
];

const DOM_PROPS: &[&str] = &[
    "textContent", "innerHTML", "outerHTML", "innerText", "outerText",
];

pub struct NoDomManipulating;

impl Rule for NoDomManipulating {
    fn name(&self) -> &'static str {
        "svelte/no-dom-manipulating"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        // 1. Collect variable NAMES bound via `bind:this={var}` on native elements.
        let mut bound_names = FxHashSet::default();
        walk_template_nodes(&ctx.ast.html, &mut |node| {
            let TemplateNode::Element(el) = node else { return };
            let is_native = el.name == "svelte:element"
                || (el.name.as_bytes().first().map_or(false, |c| c.is_ascii_lowercase())
                    && !el.name.starts_with("svelte:component")
                    && !el.name.starts_with("svelte:self"));
            if !is_native {
                return;
            }
            for attr in &el.attributes {
                let Attribute::Directive { kind: DirectiveKind::Binding, name, value, .. } = attr
                else { continue };
                if name != "this" {
                    continue;
                }
                // The value for `bind:this={var}` should reference a single identifier.
                match value {
                    AttributeValue::Expression(expr) => {
                        let n = expr.trim();
                        if !n.is_empty() && n.chars().all(is_ident_char) {
                            bound_names.insert(n.to_string());
                        }
                    }
                    AttributeValue::Concat(parts) => {
                        // `bind:this="{var}"` — one Expression part.
                        if parts.len() == 1 {
                            if let AttributeValuePart::Expression(expr) = &parts[0] {
                                let n = expr.trim();
                                if !n.is_empty() && n.chars().all(is_ident_char) {
                                    bound_names.insert(n.to_string());
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        if bound_names.is_empty() {
            return;
        }

        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;
        let scoping = semantic.scoping();

        // 2. Resolve each bound name to a symbol in the instance root scope.
        //    Only variables declared with `let`/`var` at top level qualify — this
        //    matches the vendor's `let foo` filter and avoids imports of
        //    components (which are not bound to DOM even if named the same).
        let mut bound_symbols = FxHashSet::<SymbolId>::default();
        for name in &bound_names {
            if let Some(sid) =
                scoping.find_binding(scoping.root_scope_id(), Ident::new_const(name.as_str()))
            {
                let flags = scoping.symbol_flags(sid);
                // Accept `let`/`var`/parameter-like bindings; skip `const`
                // (which is fine since bind:this requires assignable binding).
                if flags.intersects(
                    oxc::semantic::SymbolFlags::BlockScopedVariable
                        | oxc::semantic::SymbolFlags::FunctionScopedVariable,
                ) && !flags.intersects(oxc::semantic::SymbolFlags::ConstVariable)
                {
                    bound_symbols.insert(sid);
                }
            }
        }
        if bound_symbols.is_empty() {
            return;
        }

        let nodes = semantic.nodes();
        let msg = "Don't manipulate the DOM directly. The Svelte runtime can get confused if there is a difference between the actual DOM and the DOM expected by the Svelte runtime.";

        // 3. DOM method calls: `foo.remove()`, `foo?.remove()`, `(foo?.remove)()`.
        for node in nodes.iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            // Unwrap the callee: parens, chain expression.
            let callee = strip_wrappers(&ce.callee);
            let Some((base_sym, method, end_span)) =
                base_symbol_and_tail(callee, scoping, semantic)
            else {
                continue;
            };
            if !bound_symbols.contains(&base_sym) {
                continue;
            }
            if !DOM_METHODS.contains(&method) {
                continue;
            }
            let s = content_offset + ce.span.start;
            let e = content_offset + end_span.end;
            ctx.diagnostic(msg, Span::new(s, e));
        }

        // 4. DOM property assignments: `foo.textContent = 'x'`, `foo.innerHTML += 'x'`.
        for node in nodes.iter() {
            let AstKind::AssignmentExpression(ae) = node.kind() else { continue };
            let Some((base_sym, prop, end_span)) = assignment_target_tail(&ae.left, scoping, semantic)
            else {
                continue;
            };
            if !bound_symbols.contains(&base_sym) {
                continue;
            }
            if !DOM_PROPS.contains(&prop) {
                continue;
            }
            let s = content_offset + ae.span.start;
            let e = content_offset + end_span.end;
            ctx.diagnostic(msg, Span::new(s, e));
        }
    }
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Strip `ParenthesizedExpression` and `ChainExpression` wrappers to get the
/// underlying expression — e.g. `(foo?.remove)` → `foo?.remove`.
fn strip_wrappers<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(p) => strip_wrappers(&p.expression),
        Expression::ChainExpression(c) => {
            // ChainExpression's `expression` is a ChainElement; downcast to Expression.
            match &c.expression {
                oxc::ast::ast::ChainElement::CallExpression(_ce) => expr, // keep the chain as-is
                _ => expr,
            }
        }
        _ => expr,
    }
}

/// For a call callee like `foo.method` or `foo?.method`, find the method
/// name and the base identifier's symbol id. ONLY matches single-hop
/// accesses — `foo.bar.method()` and `foo.classList.remove()` are not
/// flagged since the method is called on an intermediate object, not on
/// the bound element itself.
fn base_symbol_and_tail<'a>(
    callee: &Expression<'a>,
    scoping: &oxc::semantic::Scoping,
    semantic: &'a oxc::semantic::Semantic<'a>,
) -> Option<(SymbolId, &'a str, Span)> {
    let (mem_obj, prop, end) = match callee {
        Expression::StaticMemberExpression(m) => (&m.object, m.property.name.as_str(), m.span),
        Expression::ChainExpression(c) => match &c.expression {
            oxc::ast::ast::ChainElement::StaticMemberExpression(m) => {
                (&m.object, m.property.name.as_str(), m.span)
            }
            oxc::ast::ast::ChainElement::CallExpression(ce) => match &ce.callee {
                Expression::StaticMemberExpression(m) => (&m.object, m.property.name.as_str(), m.span),
                _ => return None,
            },
            _ => return None,
        },
        _ => return None,
    };
    // Require the object to be the bound identifier directly — no intermediate
    // property access.
    let base_id = match mem_obj {
        Expression::Identifier(id) => id,
        _ => return None,
    };
    let sid = scoping.get_reference(base_id.reference_id()).symbol_id()?;
    let _ = semantic;
    Some((sid, prop, end))
}

/// Same single-hop constraint for assignment targets.
fn assignment_target_tail<'a>(
    target: &AssignmentTarget<'a>,
    scoping: &oxc::semantic::Scoping,
    semantic: &'a oxc::semantic::Semantic<'a>,
) -> Option<(SymbolId, &'a str, Span)> {
    let AssignmentTarget::StaticMemberExpression(m) = target else { return None };
    let base_id = match &m.object {
        Expression::Identifier(id) => id,
        _ => return None,
    };
    let sid = scoping.get_reference(base_id.reference_id()).symbol_id()?;
    let _ = semantic;
    Some((sid, m.property.name.as_str(), m.span))
}

// Keep `MemberExpression` / `SimpleAssignmentTarget` imports alive in case we
// later need to match patterns that use them directly.
#[allow(dead_code)]
fn _unused(_: MemberExpression<'_>, _: SimpleAssignmentTarget<'_>) {}
