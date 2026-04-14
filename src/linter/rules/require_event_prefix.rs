//! `svelte/require-event-prefix` — require event handler props to use the `on` prefix.

use crate::linter::{LintContext, Rule};
use oxc::ast::ast::{
    Expression, PropertyKey, Statement, TSSignature, TSType, TSTypeName,
    VariableDeclarationKind,
};
use oxc::span::Span;

pub struct RequireEventPrefix;

impl Rule for RequireEventPrefix {
    fn name(&self) -> &'static str {
        "svelte/require-event-prefix"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let Some(script) = ctx.ast.instance.as_ref() else { return };
        if !matches!(script.lang.as_deref(), Some("ts" | "typescript" | "TS" | "Typescript" | "TypeScript")) {
            return;
        }
        if !script.content.contains("$props") { return; }
        let Some(semantic) = ctx.instance_semantic else { return };
        let content_offset = ctx.instance_content_offset;

        let check_async = ctx
            .config
            .options
            .as_ref()
            .and_then(|v| v.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.get("checkAsyncFunctions"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        for stmt in &semantic.nodes().program().body {
            let Statement::VariableDeclaration(vd) = stmt else { continue };
            if !matches!(vd.kind, VariableDeclarationKind::Let | VariableDeclarationKind::Const | VariableDeclarationKind::Var) {
                continue;
            }
            for d in &vd.declarations {
                let is_props_call = d.init.as_ref().map_or(false, |init| match init {
                    Expression::CallExpression(ce) => matches!(
                        &ce.callee,
                        Expression::Identifier(id) if id.name == "$props"
                    ),
                    _ => false,
                });
                if !is_props_call { continue; }
                let Some(ann) = d.type_annotation.as_deref() else { continue };
                check_props_type(ctx, content_offset, semantic, &ann.type_annotation, check_async);
            }
        }
    }
}

fn check_props_type(
    ctx: &mut LintContext<'_>,
    content_offset: u32,
    semantic: &oxc::semantic::Semantic<'_>,
    ty: &TSType<'_>,
    check_async: bool,
) {
    match ty {
        TSType::TSTypeLiteral(lit) => {
            for sig in &lit.members {
                check_signature(ctx, content_offset, sig, check_async);
            }
        }
        TSType::TSTypeReference(tr) => {
            let name = match &tr.type_name {
                TSTypeName::IdentifierReference(id) => id.name.as_str(),
                _ => return,
            };
            for stmt in &semantic.nodes().program().body {
                if let Statement::TSInterfaceDeclaration(iface) = stmt {
                    if iface.id.name == name {
                        for sig in &iface.body.body {
                            check_signature(ctx, content_offset, sig, check_async);
                        }
                    }
                } else if let Statement::TSTypeAliasDeclaration(alias) = stmt {
                    if alias.id.name == name {
                        check_props_type(ctx, content_offset, semantic, &alias.type_annotation, check_async);
                    }
                }
            }
        }
        TSType::TSParenthesizedType(p) => {
            check_props_type(ctx, content_offset, semantic, &p.type_annotation, check_async);
        }
        _ => {}
    }
}

fn check_signature(
    ctx: &mut LintContext<'_>,
    content_offset: u32,
    sig: &TSSignature<'_>,
    check_async: bool,
) {
    match sig {
        TSSignature::TSMethodSignature(m) => {
            let Some((name, span)) = key_name_and_span(&m.key) else { return };
            if name.starts_with("on") { return; }
            if !check_async && method_returns_promise(m) { return; }
            report(ctx, content_offset, span);
        }
        TSSignature::TSPropertySignature(p) => {
            let Some(ann) = &p.type_annotation else { return };
            if !matches!(&ann.type_annotation, TSType::TSFunctionType(_)) { return; }
            let Some((name, span)) = key_name_and_span(&p.key) else { return };
            if name.starts_with("on") { return; }
            // Vendor's `isFunctionAsync` only matches TSMethodSignature, so
            // `() => Promise<void>` is always reported here — mirror that.
            report(ctx, content_offset, span);
        }
        _ => {}
    }
}

fn key_name_and_span<'a>(key: &'a PropertyKey<'a>) -> Option<(&'a str, Span)> {
    match key {
        PropertyKey::StaticIdentifier(id) => Some((id.name.as_str(), id.span)),
        PropertyKey::StringLiteral(l) => Some((l.value.as_str(), l.span)),
        _ => None,
    }
}

fn method_returns_promise(m: &oxc::ast::ast::TSMethodSignature<'_>) -> bool {
    let Some(rt) = &m.return_type else { return false };
    let TSType::TSTypeReference(tr) = &rt.type_annotation else { return false };
    matches!(&tr.type_name, TSTypeName::IdentifierReference(id) if id.name == "Promise")
}

fn report(ctx: &mut LintContext<'_>, content_offset: u32, key_span: Span) {
    let s = content_offset + key_span.start;
    let e = content_offset + key_span.end;
    ctx.diagnostic(
        "Component event name must start with \"on\".",
        Span::new(s, e),
    );
}
