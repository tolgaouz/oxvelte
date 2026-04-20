//! Svelte AST node definitions.
//!
//! Template types carry a `'a` lifetime so future commits can replace
//! `String`-typed expression / declaration fields with borrowed oxc AST
//! nodes (`oxc::ast::ast::Expression<'a>`, `BindingPattern<'a>`, …) that
//! live in a shared `oxc::allocator::Allocator`.
//!
//! In this commit the fields are still `String` — the `'a` is held via
//! `PhantomData<&'a ()>` so every consumer already propagates the lifetime
//! through the AST type. `Attribute`, `AttributeValue`, and
//! `AttributeValuePart` stay non-generic in this commit to avoid needing
//! `PhantomData` inside tuple variants (which would force updating every
//! `AttributeValue::Expression(expr, _)` / `::Expression(expr, _)` pattern
//! match in every rule). Those types will become generic when we actually
//! store `Expression<'a>` in them, in a follow-up.

use oxc::span::Span;
use serde::Serialize;
use std::marker::PhantomData;

#[derive(Debug, Clone, Serialize)]
pub struct SvelteAst<'a> {
    pub html: Fragment<'a>,
    pub instance: Option<Script>,
    pub module: Option<Script>,
    pub css: Option<Style>,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fragment<'a> {
    pub nodes: Vec<TemplateNode<'a>>,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TemplateNode<'a> {
    Text(Text),
    Element(Element<'a>),
    MustacheTag(MustacheTag<'a>),
    RawMustacheTag(RawMustacheTag<'a>),
    DebugTag(DebugTag<'a>),
    ConstTag(ConstTag<'a>),
    RenderTag(RenderTag<'a>),
    Comment(Comment),
    IfBlock(IfBlock<'a>),
    EachBlock(EachBlock<'a>),
    AwaitBlock(AwaitBlock<'a>),
    KeyBlock(KeyBlock<'a>),
    SnippetBlock(SnippetBlock<'a>),
}

#[derive(Debug, Clone, Serialize)]
pub struct Text {
    pub data: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Element<'a> {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<TemplateNode<'a>>,
    pub self_closing: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Attribute {
    NormalAttribute { name: String, value: AttributeValue, span: Span },
    Spread { span: Span },
    Directive { kind: DirectiveKind, name: String, modifiers: Vec<String>, value: AttributeValue, span: Span },
}

#[derive(Debug, Clone, Serialize)]
pub enum DirectiveKind {
    EventHandler, Binding, Class, StyleDirective, Use,
    Transition, In, Out, Animate, Let,
}

#[derive(Debug, Clone, Serialize)]
pub enum AttributeValue {
    Static(String),
    Expression(String),
    Concat(Vec<AttributeValuePart>),
    True,
}

#[derive(Debug, Clone, Serialize)]
pub enum AttributeValuePart {
    Static(String),
    Expression(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct MustacheTag<'a> {
    pub expression: String,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMustacheTag<'a> {
    pub expression: String,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugTag<'a> {
    pub identifiers: Vec<String>,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstTag<'a> {
    pub declaration: String,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderTag<'a> {
    pub expression: String,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comment { pub data: String, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct IfBlock<'a> {
    pub test: String,
    pub consequent: Fragment<'a>,
    pub alternate: Option<Box<TemplateNode<'a>>>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct EachBlock<'a> {
    pub expression: String,
    pub context: String,
    pub index: Option<String>,
    pub key: Option<String>,
    pub body: Fragment<'a>,
    pub fallback: Option<Fragment<'a>>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct AwaitBlock<'a> {
    pub expression: String,
    pub pending: Option<Fragment<'a>>,
    pub then: Option<Fragment<'a>>,
    pub then_binding: Option<String>,
    pub catch: Option<Fragment<'a>>,
    pub catch_binding: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyBlock<'a> {
    pub expression: String,
    pub body: Fragment<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnippetBlock<'a> {
    pub name: String,
    pub params: String,
    pub body: Fragment<'a>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Script {
    pub content: String,
    pub module: bool,
    pub lang: Option<String>,
    /// True when the `<script>` open tag has a boolean `strictEvents` attribute.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strict_events: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Style {
    pub content: String,
    pub lang: Option<String>,
    pub span: Span,
}
