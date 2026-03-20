//! Svelte AST node definitions.

use oxc::span::Span;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SvelteAst {
    pub html: Fragment,
    pub instance: Option<Script>,
    pub module: Option<Script>,
    pub css: Option<Style>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Fragment {
    pub nodes: Vec<TemplateNode>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TemplateNode {
    Text(Text),
    Element(Element),
    MustacheTag(MustacheTag),
    RawMustacheTag(RawMustacheTag),
    DebugTag(DebugTag),
    ConstTag(ConstTag),
    RenderTag(RenderTag),
    Comment(Comment),
    IfBlock(IfBlock),
    EachBlock(EachBlock),
    AwaitBlock(AwaitBlock),
    KeyBlock(KeyBlock),
    SnippetBlock(SnippetBlock),
}

#[derive(Debug, Clone, Serialize)]
pub struct Text {
    pub data: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Element {
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<TemplateNode>,
    pub self_closing: bool,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Attribute {
    NormalAttribute { name: String, value: AttributeValue, span: Span },
    Spread { span: Span },
    Directive { kind: DirectiveKind, name: String, modifiers: Vec<String>, span: Span },
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
pub struct MustacheTag { pub expression: String, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct RawMustacheTag { pub expression: String, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct DebugTag { pub identifiers: Vec<String>, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct ConstTag { pub declaration: String, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct RenderTag { pub expression: String, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct Comment { pub data: String, pub span: Span }

#[derive(Debug, Clone, Serialize)]
pub struct IfBlock {
    pub test: String,
    pub consequent: Fragment,
    pub alternate: Option<Box<TemplateNode>>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct EachBlock {
    pub expression: String,
    pub context: String,
    pub index: Option<String>,
    pub key: Option<String>,
    pub body: Fragment,
    pub fallback: Option<Fragment>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct AwaitBlock {
    pub expression: String,
    pub pending: Option<Fragment>,
    pub then: Option<Fragment>,
    pub then_binding: Option<String>,
    pub catch: Option<Fragment>,
    pub catch_binding: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyBlock {
    pub expression: String,
    pub body: Fragment,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnippetBlock {
    pub name: String,
    pub params: String,
    pub body: Fragment,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Script {
    pub content: String,
    pub module: bool,
    pub lang: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Style {
    pub content: String,
    pub lang: Option<String>,
    pub span: Span,
}
