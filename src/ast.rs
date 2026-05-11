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
    /// Template block/control tag spans (`{#if ...}`, `{:else}`, `{/if}`, …)
    /// collected by the parser. These are skipped from serialization because
    /// they are linter metadata, not part of the public Svelte AST shape.
    #[serde(skip)]
    pub template_tag_spans: Vec<TemplateTagSpan>,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemplateTagSpan {
    pub span: Span,
    pub has_expression: bool,
    pub check_closing: bool,
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
    /// Span of the element name in the opening tag, excluding the leading `<`.
    #[serde(skip)]
    pub name_span: Span,
    pub attributes: Vec<Attribute>,
    #[serde(skip)]
    pub attribute_meta: Vec<AttributeMeta<'a>>,
    pub children: Vec<TemplateNode<'a>>,
    pub self_closing: bool,
    /// Full element span: from `<` of the opening tag through `>` of the
    /// end tag (or `/>` for self-closing / void elements).
    pub span: Span,
    /// Byte offset of the `>` that closes the start tag. For self-closing
    /// and void elements this is the `>` of `/>`. Format-level rules
    /// (`html-closing-bracket-spacing`, `max-attributes-per-line`, …) use
    /// this to scope trailing-whitespace inspection to the start tag
    /// without walking the element source to find the bracket themselves.
    #[serde(skip)]
    pub start_tag_end: u32,
    /// Span of the `</name>` end tag, from `<` through `>`. `None` for
    /// self-closing and void elements.
    #[serde(skip)]
    pub end_tag_span: Option<Span>,
    /// True when this element was left on the parser's open-node stack at
    /// EOF and is *not* the innermost unclosed entry. the `end` of such nodes
    /// are left at `-1` (its initial sentinel value); only the topmost gets
    /// adjusted to `template.length`. Modern + legacy serializers consult this
    /// and emit `end: -1` to match.
    #[serde(skip)]
    pub unclosed_at_eof_outer: bool,
}

/// Structural classification of a template element.
///
/// Mirrors `svelte-eslint-parser`'s `SvelteElement.kind` discriminator: every
/// element falls into one of three buckets, and the `SvelteSpecial` bucket
/// further distinguishes which `<svelte:*>` tag we're looking at. Linter
/// rules that need to filter "is this a real HTML element?" should use
/// [`Element::kind`] / [`ElementKind`] rather than re-deriving the answer
/// from `el.name` patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    /// Regular HTML element — lowercase first letter, no `:`, no `.`.
    /// (e.g. `<div>`, `<span>`, `<slot>`.)
    Html,
    /// Component reference — PascalCase first letter or dotted member access.
    /// (e.g. `<MyComp>`, `<foo.Bar>`.)
    Component,
    /// `<svelte:*>` special tag.
    SvelteSpecial(SvelteSpecial),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvelteSpecial {
    Component, // <svelte:component>
    Self_,     // <svelte:self>
    Element,   // <svelte:element>
    Fragment,  // <svelte:fragment>
    Head,      // <svelte:head>
    Body,      // <svelte:body>
    Window,    // <svelte:window>
    Document,  // <svelte:document>
    Options,   // <svelte:options>
    Boundary,  // <svelte:boundary>
    /// Catch-all for `svelte:foo` tags the parser produces but we don't
    /// model explicitly. Rules that want to be conservative can treat
    /// `Unknown` like `Component` (i.e. don't apply HTML-only logic).
    Unknown,
}

impl ElementKind {
    /// Classify a template element by its tag name.
    pub fn classify(name: &str) -> Self {
        if let Some(suffix) = name.strip_prefix("svelte:") {
            return ElementKind::SvelteSpecial(match suffix {
                "component" => SvelteSpecial::Component,
                "self" => SvelteSpecial::Self_,
                "element" => SvelteSpecial::Element,
                "fragment" => SvelteSpecial::Fragment,
                "head" => SvelteSpecial::Head,
                "body" => SvelteSpecial::Body,
                "window" => SvelteSpecial::Window,
                "document" => SvelteSpecial::Document,
                "options" => SvelteSpecial::Options,
                "boundary" => SvelteSpecial::Boundary,
                _ => SvelteSpecial::Unknown,
            });
        }
        match name.chars().next() {
            Some(c) if c.is_ascii_uppercase() => ElementKind::Component,
            _ if name.contains('.') => ElementKind::Component,
            _ => ElementKind::Html,
        }
    }

    pub fn is_html(self) -> bool {
        matches!(self, ElementKind::Html)
    }
    pub fn is_component(self) -> bool {
        matches!(self, ElementKind::Component)
    }
    pub fn is_svelte_special(self) -> bool {
        matches!(self, ElementKind::SvelteSpecial(_))
    }
}

impl<'a> Element<'a> {
    /// Returns the structural kind of this element. See [`ElementKind`].
    pub fn kind(&self) -> ElementKind {
        ElementKind::classify(&self.name)
    }

    /// Typed expression AST for the attribute at index `idx`, when the
    /// attribute's value is a single expression mustache (`name={expr}`,
    /// shorthand `{name}`, or a directive expression). Returns `None` for
    /// literal values, `Concat` values, and parse failures. Use
    /// [`Element::attribute_part_expression_ast`] for `Concat` parts.
    pub fn attribute_expression_ast(
        &self,
        idx: usize,
    ) -> Option<&'a oxc::ast::ast::Expression<'a>> {
        self.attribute_meta.get(idx).and_then(|m| m.expression_ast)
    }

    /// Typed expression AST for a single mustache `{expr}` inside the
    /// `Concat` value of attribute `attr_idx`, where `part_idx` is the index
    /// into `AttributeValuePart`s. Returns `None` for static parts and parse
    /// failures.
    pub fn attribute_part_expression_ast(
        &self,
        attr_idx: usize,
        part_idx: usize,
    ) -> Option<&'a oxc::ast::ast::Expression<'a>> {
        self.attribute_meta
            .get(attr_idx)
            .and_then(|m| m.parts.get(part_idx))
            .and_then(|p| p.expression_ast)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Attribute {
    NormalAttribute {
        name: String,
        value: AttributeValue,
        span: Span,
    },
    Spread {
        span: Span,
    },
    Directive {
        kind: DirectiveKind,
        name: String,
        modifiers: Vec<String>,
        value: AttributeValue,
        span: Span,
    },
}

#[derive(Debug, Clone, Serialize)]
pub enum DirectiveKind {
    EventHandler,
    Binding,
    Class,
    StyleDirective,
    Use,
    Transition,
    In,
    Out,
    Animate,
    Let,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributeQuote {
    Double,
    Single,
}

#[derive(Debug, Clone)]
pub struct AttributeMeta<'a> {
    pub name_span: Span,
    pub directive_subject_span: Option<Span>,
    pub value_span: Option<Span>,
    /// Full value token span, including surrounding quotes for quoted values
    /// and surrounding braces for single mustache expression values.
    pub value_full_span: Option<Span>,
    pub quote: Option<AttributeQuote>,
    /// Span from the end of the attribute/directive key through the first
    /// byte of the value token. For `foo = "bar"`, this covers ` = `.
    pub equals_span: Option<Span>,
    pub expression_span: Option<Span>,
    pub mustache_span: Option<Span>,
    /// Typed AST for the attribute's expression value. Populated when the
    /// attribute is `name={expr}`, shorthand `{name}`, or a directive whose
    /// value is a single expression. `None` when the value is a literal,
    /// `Concat`, or the expression text failed to parse as JS. Mirrors
    /// `MustacheTag::expression_ast`.
    pub expression_ast: Option<&'a oxc::ast::ast::Expression<'a>>,
    pub parts: Vec<AttributePartMeta<'a>>,
}

#[derive(Debug, Clone)]
pub struct AttributePartMeta<'a> {
    pub span: Span,
    pub expression_span: Option<Span>,
    pub mustache_span: Option<Span>,
    /// Typed AST for this part of a `Concat` value, when the part is an
    /// expression mustache. `None` for static-text parts and parse failures.
    pub expression_ast: Option<&'a oxc::ast::ast::Expression<'a>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MustacheTag<'a> {
    pub expression: String,
    pub span: Span,
    #[serde(skip)]
    pub expression_span: Span,
    /// Typed AST of the mustache's inner expression, parsed into a shared
    /// `oxc::allocator::Allocator` during template parsing. `None` when the
    /// expression text failed to parse as JS (the rule layer then falls
    /// back to `expression` as raw text).
    #[serde(skip)]
    pub expression_ast: Option<&'a oxc::ast::ast::Expression<'a>>,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RawMustacheTag<'a> {
    pub expression: String,
    pub span: Span,
    #[serde(skip)]
    pub expression_span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugTag<'a> {
    pub identifiers: Vec<String>,
    #[serde(skip)]
    pub identifier_spans: Vec<Span>,
    pub span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConstTag<'a> {
    pub declaration: String,
    pub span: Span,
    #[serde(skip)]
    pub declaration_span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenderTag<'a> {
    pub expression: String,
    pub span: Span,
    #[serde(skip)]
    pub expression_span: Span,
    #[serde(skip)]
    pub _phantom: PhantomData<&'a ()>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comment {
    pub data: String,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct IfBlock<'a> {
    pub test: String,
    #[serde(skip)]
    pub test_span: Span,
    #[serde(skip)]
    pub header_span: Span,
    #[serde(skip)]
    pub elseif: bool,
    pub consequent: Fragment<'a>,
    pub alternate: Option<Box<TemplateNode<'a>>>,
    pub span: Span,
    /// See [`Element::unclosed_at_eof_outer`].
    #[serde(skip)]
    pub unclosed_at_eof_outer: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EachBlock<'a> {
    pub expression: String,
    #[serde(skip)]
    pub expression_span: Span,
    pub context: String,
    #[serde(skip)]
    pub context_span: Span,
    pub index: Option<String>,
    #[serde(skip)]
    pub index_span: Option<Span>,
    pub key: Option<String>,
    #[serde(skip)]
    pub key_span: Option<Span>,
    #[serde(skip)]
    pub header_span: Span,
    pub body: Fragment<'a>,
    pub fallback: Option<Fragment<'a>>,
    pub span: Span,
    /// See [`Element::unclosed_at_eof_outer`].
    #[serde(skip)]
    pub unclosed_at_eof_outer: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AwaitBlock<'a> {
    pub expression: String,
    #[serde(skip)]
    pub expression_span: Span,
    pub pending: Option<Fragment<'a>>,
    pub then: Option<Fragment<'a>>,
    pub then_binding: Option<String>,
    #[serde(skip)]
    pub then_binding_span: Option<Span>,
    pub catch: Option<Fragment<'a>>,
    pub catch_binding: Option<String>,
    #[serde(skip)]
    pub catch_binding_span: Option<Span>,
    pub span: Span,
    /// See [`Element::unclosed_at_eof_outer`].
    #[serde(skip)]
    pub unclosed_at_eof_outer: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyBlock<'a> {
    pub expression: String,
    #[serde(skip)]
    pub expression_span: Span,
    pub body: Fragment<'a>,
    pub span: Span,
    /// See [`Element::unclosed_at_eof_outer`].
    #[serde(skip)]
    pub unclosed_at_eof_outer: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnippetBlock<'a> {
    pub name: String,
    #[serde(skip)]
    pub name_span: Span,
    pub type_params: Option<String>,
    #[serde(skip)]
    pub type_params_span: Option<Span>,
    pub params: String,
    #[serde(skip)]
    pub params_span: Option<Span>,
    pub body: Fragment<'a>,
    pub span: Span,
    /// See [`Element::unclosed_at_eof_outer`].
    #[serde(skip)]
    pub unclosed_at_eof_outer: bool,
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
    #[serde(skip)]
    pub attrs_span: Span,
    #[serde(skip)]
    pub content_span: Span,
}

#[derive(Debug, Clone, Serialize)]
pub struct Style {
    pub content: String,
    pub lang: Option<String>,
    pub span: Span,
    #[serde(skip)]
    pub attrs_span: Span,
    #[serde(skip)]
    pub content_span: Span,
}
