//! `svelte/no-unnecessary-state-wrap` — disallow wrapping values that are already reactive with `$state`.
//! ⭐ Recommended 💡
//!
//! Svelte's reactive classes (SvelteSet, SvelteMap, etc.) are already reactive
//! and don't need `$state()` wrapping.

use crate::linter::{walk_template_nodes, LintContext, Rule};
use crate::ast::{Attribute, AttributeValue, DirectiveKind, Fragment, TemplateNode};
use oxc::ast::ast::{Argument, Expression, ImportDeclarationSpecifier, ModuleExportName, Statement};
use oxc::ast::AstKind;
use oxc::semantic::SymbolId;
use oxc::span::Span;
use rustc_hash::FxHashSet;

const REACTIVE_CLASSES: &[&str] = &[
    "SvelteSet", "SvelteMap", "SvelteURL", "SvelteURLSearchParams",
    "SvelteDate", "MediaQuery",
];

pub struct NoUnnecessaryStateWrap;

impl Rule for NoUnnecessaryStateWrap {
    fn name(&self) -> &'static str {
        "svelte/no-unnecessary-state-wrap"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        let opts = ctx.config.options.as_ref().and_then(|o| o.as_array()).and_then(|a| a.first());
        let additional: Vec<String> = opts
            .and_then(|o| o.get("additionalReactiveClasses"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|c| c.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let allow_reassign = opts
            .and_then(|o| o.get("allowReassign"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Build a map: local name → original name.
        // Seed with bare `SvelteSet` / `SvelteMap` etc. (direct use without import alias)
        // and with any `additionalReactiveClasses` names (used directly).
        let mut name_map: Vec<(String, String)> = REACTIVE_CLASSES.iter()
            .map(|s| (s.to_string(), s.to_string()))
            .chain(additional.iter().cloned().map(|s| (s.clone(), s)))
            .collect();
        // Also aliased imports from `svelte/*` or additional classes' custom modules.
        let nodes = semantic.nodes();
        let program = nodes.program();
        for stmt in &program.body {
            let Statement::ImportDeclaration(imp) = stmt else { continue };
            let src = imp.source.value.as_str();
            let is_svelte = src.starts_with("svelte/") || src == "svelte";
            let Some(specifiers) = &imp.specifiers else { continue };
            for spec in specifiers {
                let ImportDeclarationSpecifier::ImportSpecifier(s) = spec else { continue };
                let imported = match &s.imported {
                    ModuleExportName::IdentifierName(n) => n.name.as_str(),
                    ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                    ModuleExportName::StringLiteral(l) => l.value.as_str(),
                };
                let local = s.local.name.as_str();
                let is_reactive = (is_svelte && REACTIVE_CLASSES.contains(&imported))
                    || additional.iter().any(|c| c == imported);
                if is_reactive && local != imported {
                    name_map.push((local.to_string(), imported.to_string()));
                }
            }
        }

        let scoping = semantic.scoping();
        for node in nodes.iter() {
            let AstKind::CallExpression(ce) = node.kind() else { continue };
            let Expression::Identifier(callee) = &ce.callee else { continue };
            if callee.name != "$state" {
                continue;
            }
            let Some(first_arg) = ce.arguments.first() else { continue };
            let Argument::NewExpression(new_expr) = first_arg else { continue };
            let Expression::Identifier(class_id) = &new_expr.callee else { continue };
            let Some((_, original)) = name_map.iter().find(|(l, _)| l == class_id.name.as_str()) else {
                continue;
            };

            // Walk up to the enclosing VariableDeclarator. If none, skip.
            let call_node_id = node.id();
            let mut cursor = call_node_id;
            let mut decl_kind: Option<&'static str> = None; // "const" or "let"
            let mut decl_symbol: Option<SymbolId> = None;
            loop {
                let parent_id = nodes.parent_id(cursor);
                if parent_id == cursor {
                    break;
                }
                let parent_kind = nodes.kind(parent_id);
                if let AstKind::VariableDeclarator(_vd) = parent_kind {
                    // Find the VariableDeclaration to get its kind.
                    let gp_id = nodes.parent_id(parent_id);
                    if let AstKind::VariableDeclaration(vd_decl) = nodes.kind(gp_id) {
                        decl_kind = Some(match vd_decl.kind {
                            oxc::ast::ast::VariableDeclarationKind::Const => "const",
                            oxc::ast::ast::VariableDeclarationKind::Let => "let",
                            _ => break,
                        });
                        // Resolve the binding symbol id.
                        let vd = match parent_kind {
                            AstKind::VariableDeclarator(vd) => vd,
                            _ => break,
                        };
                        if let oxc::ast::ast::BindingPattern::BindingIdentifier(id) = &vd.id {
                            decl_symbol = scoping.get_binding(scoping.root_scope_id(), oxc::span::Ident::new_const(id.name.as_str()));
                        }
                    }
                    break;
                }
                cursor = parent_id;
            }
            let Some(kind) = decl_kind else { continue };

            let should_flag = match kind {
                "const" => true,
                "let" => {
                    if !allow_reassign {
                        true
                    } else {
                        // `let` + allowReassign: skip if the var IS reassigned.
                        let reassigned = decl_symbol
                            .map(|sid| is_symbol_reassigned(sid, scoping, nodes, &ctx.ast.html))
                            .unwrap_or(false);
                        !reassigned
                    }
                }
                _ => false,
            };
            if !should_flag {
                continue;
            }

            let s = content_offset + callee.span.start;
            let e = content_offset + callee.span.end + 1; // include `(`
            ctx.diagnostic(
                format!("{} is already reactive, $state wrapping is unnecessary.", original),
                Span::new(s, e),
            );
        }
    }
}

/// Has this symbol been reassigned anywhere (JS write reference, or a template
/// `bind:` directive that would write through to this name)?
fn is_symbol_reassigned<'a>(
    sid: SymbolId,
    scoping: &'a oxc::semantic::Scoping,
    _nodes: &'a oxc::semantic::AstNodes<'a>,
    html: &'a Fragment,
) -> bool {
    if scoping.get_resolved_references(sid).any(|r| r.is_write()) {
        return true;
    }
    let name = scoping.symbol_name(sid);
    let mut found = false;
    walk_template_nodes(html, &mut |node| {
        if found { return; }
        let TemplateNode::Element(el) = node else { return };
        for attr in &el.attributes {
            let Attribute::Directive { kind: DirectiveKind::Binding, name: dir_name, value, .. } = attr
                else { continue };
            // `bind:name` shorthand — the directive name is the bound symbol.
            if dir_name == name {
                found = true;
                return;
            }
            // `bind:anything={name}` / nested member writing through name.
            if let AttributeValue::Expression(text) = value {
                let trimmed = text.trim();
                let base = trimmed.split(|c: char| c == '.' || c == '[').next().unwrap_or(trimmed);
                if base == name {
                    found = true;
                    return;
                }
            }
        }
    });
    found
}

// Ensure we import FxHashSet to keep the module compile clean even when unused.
#[allow(dead_code)]
fn _keep_imports() -> FxHashSet<SymbolId> { FxHashSet::default() }
