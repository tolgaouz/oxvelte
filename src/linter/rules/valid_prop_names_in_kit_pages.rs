//! `svelte/valid-prop-names-in-kit-pages` — ensure exported props in SvelteKit pages
//! use valid names (`data`, `form`, `snapshot`).
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{
    BindingPattern, Declaration, Expression, ExportNamedDeclaration,
    Statement, VariableDeclarationKind,
};
use oxc::span::Span;

const VALID_KIT_PROPS: &[&str] = &["data", "errors", "form", "params", "snapshot"];

const VALID_KIT_PROPS_SVELTE5: &[&str] = &["data", "errors", "form", "params", "snapshot", "children"];

pub struct ValidPropNamesInKitPages;

impl Rule for ValidPropNamesInKitPages {
    fn name(&self) -> &'static str {
        "svelte/valid-prop-names-in-kit-pages"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(file_path) = &ctx.file_path else { return; };
        let fname = file_path.rsplit('/').next().unwrap_or(file_path);
        let fname = fname.rsplit('\\').next().unwrap_or(fname);
        if fname != "+page.svelte" && fname != "+layout.svelte" && fname != "+error.svelte" {
            return;
        }
        if let Some(routes_dir) = ctx
            .config
            .settings
            .as_ref()
            .and_then(|s| s.get("svelte"))
            .and_then(|s| s.get("kit"))
            .and_then(|s| s.get("files"))
            .and_then(|s| s.get("routes"))
            .and_then(|s| s.as_str())
        {
            if !file_path.contains(routes_dir) {
                return;
            }
        }

        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        for stmt in &semantic.nodes().program().body {
            match stmt {
                // `export let name;` / `export let name = init;` — Svelte 3/4 props.
                Statement::ExportNamedDeclaration(exp) => {
                    check_export_named(ctx, content_offset, exp);
                }
                // `let { ... } = $props();` — Svelte 5 props.
                Statement::VariableDeclaration(vd) if vd.kind == VariableDeclarationKind::Let => {
                    for d in &vd.declarations {
                        let is_props_call = d.init.as_ref().map_or(false, |init| match init {
                            Expression::CallExpression(ce) => matches!(
                                &ce.callee,
                                Expression::Identifier(id) if id.name == "$props"
                            ),
                            _ => false,
                        });
                        if is_props_call {
                            report_pattern_names(ctx, content_offset, &d.id, VALID_KIT_PROPS_SVELTE5);
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn check_export_named<'a>(
    ctx: &mut LintContext<'_>,
    content_offset: u32,
    exp: &'a ExportNamedDeclaration<'a>,
) {
    let Some(decl) = &exp.declaration else { return };
    let Declaration::VariableDeclaration(vd) = decl else { return };
    if !matches!(vd.kind, VariableDeclarationKind::Let | VariableDeclarationKind::Var) {
        return;
    }
    for d in &vd.declarations {
        report_pattern_names(ctx, content_offset, &d.id, VALID_KIT_PROPS);
    }
}

fn report_pattern_names<'a>(
    ctx: &mut LintContext<'_>,
    content_offset: u32,
    pat: &BindingPattern<'a>,
    valid: &[&str],
) {
    match pat {
        BindingPattern::BindingIdentifier(id) => {
            if !valid.contains(&id.name.as_str()) {
                report(ctx, content_offset, id.name.as_str(), id.span);
            }
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                let key_name = match &prop.key {
                    oxc::ast::ast::PropertyKey::StaticIdentifier(id) => id.name.as_str(),
                    oxc::ast::ast::PropertyKey::StringLiteral(l) => l.value.as_str(),
                    _ => continue,
                };
                if !valid.contains(&key_name) {
                    report(ctx, content_offset, key_name, prop.key.span());
                }
            }
            if let Some(rest) = &obj.rest {
                if let BindingPattern::BindingIdentifier(id) = &rest.argument {
                    if !valid.contains(&id.name.as_str()) {
                        report(ctx, content_offset, id.name.as_str(), id.span);
                    }
                }
            }
        }
        BindingPattern::ArrayPattern(_) => {
            // Array destructuring of $props is invalid anyway; skip.
        }
        BindingPattern::AssignmentPattern(inner) => {
            // `{ custom = 'default' }` — walk into the left-hand pattern.
            report_pattern_names(ctx, content_offset, &inner.left, valid);
        }
    }
}

fn report(ctx: &mut LintContext<'_>, content_offset: u32, _name: &str, span: Span) {
    let s = content_offset + span.start;
    let e = content_offset + span.end;
    ctx.diagnostic(
        "disallow props other than data or errors in SvelteKit page components.",
        Span::new(s, e),
    );
}

// We accidentally introduced `BindingPatternKind` via oxc's re-export path;
// if it disappears, this import needs updating.
use oxc::span::GetSpan;
