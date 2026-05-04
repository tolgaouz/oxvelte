//! Svelte template parser.
//!
//! Parses the template portion of a `.svelte` file (everything outside `<script>` and
//! `<style>` blocks) into a tree of [`TemplateNode`]s.

use crate::ast::*;
use crate::parser::scanner;
use oxc::allocator::Allocator;
use oxc::span::Span;
use oxc_diagnostics::OxcDiagnostic;
use std::marker::PhantomData;

/// Parse a template source string into a [`Fragment`]. The `allocator`
/// owns any pre-parsed template expression nodes stored on template AST
/// nodes (e.g. `MustacheTag.expression_ast`). It must outlive the
/// returned `Fragment`.
pub fn parse_fragment<'a>(
    source: &'a str,
    allocator: &'a Allocator,
) -> Result<Fragment<'a>, OxcDiagnostic> {
    let mut parser = TemplateParser::new(source, allocator);
    parser.parse_root_fragment()
}

pub fn parse_fragment_with_errors<'a>(
    source: &'a str,
    allocator: &'a Allocator,
) -> (Fragment<'a>, Vec<OxcDiagnostic>) {
    let mut parser = TemplateParser::new(source, allocator);
    match parser.parse_root_fragment() {
        Ok(fragment) => (fragment, parser.errors),
        Err(error) => {
            parser.errors.push(error);
            (
                Fragment {
                    nodes: Vec::new(),
                    span: Span::new(0, source.len() as u32),
                    _phantom: PhantomData,
                },
                parser.errors,
            )
        }
    }
}

/// The template parser state machine.
struct TemplateParser<'a> {
    source: &'a str,
    pos: usize,
    allocator: &'a Allocator,
    errors: Vec<OxcDiagnostic>,
    /// Stack of in-progress fragment node lists, mirroring vendor Svelte's
    /// `Parser.fragments`. The innermost frame is `last_mut()`; node-producing
    /// parse steps push through `append()` instead of returning nodes via
    /// stack-allocated `Vec`s.
    fragments: Vec<Vec<TemplateNode<'a>>>,
    /// Mixed stack of pending open nodes (regular elements with children and
    /// flat blocks). Mirrors vendor Svelte's `Parser.stack`. Each entry pairs
    /// with at least one fragment frame on `fragments`.
    open_nodes: Vec<OpenNode<'a>>,
    /// `open_nodes.len()` snapshots taken at each `parse_fragment_frame`
    /// entry. Used to scope "parent element" lookups and block/element close
    /// finalization to the current frame — entries opened in outer frames
    /// (e.g., a `<div>` outside a `{#if}`) must not leak into placement
    /// diagnostics or implicit-close decisions inside the inner frame's body.
    frame_checkpoints: Vec<usize>,
    seen_root_meta_tags: Vec<String>,
    in_svelte_head_context: bool,
    svelte_self_allowed_depth: usize,
    shadowroot_template_depth: usize,
    reported_unclosed_eof: bool,
    last_auto_closed_tag: Option<AutoClosedTag>,
}

enum FragmentStep<'a> {
    Node(TemplateNode<'a>),
    /// The dispatch step encountered a token that can't be handled inside
    /// the current frame (an unmatched block close, a block continuation
    /// that no flat block on the stack claimed, etc.). The reason payload
    /// isn't needed — only the fact that the loop should break is.
    Exit,
    Continue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockContinuation {
    Else,
    ElseIf,
    InvalidElseIf,
    Then,
    Catch,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockClose {
    If,
    Each,
    Await,
    Key,
    Snippet,
}

impl BlockClose {
    fn tag(self) -> &'static str {
        match self {
            Self::If => "if",
            Self::Each => "each",
            Self::Await => "await",
            Self::Key => "key",
            Self::Snippet => "snippet",
        }
    }

    fn unclosed_message(self) -> &'static str {
        match self {
            Self::If => "Unclosed if block",
            Self::Each => "Unclosed each block",
            Self::Await => "Unclosed await block",
            Self::Key => "Unclosed key block",
            Self::Snippet => "Unclosed snippet block",
        }
    }
}

/// Frame argument distinguishing root-fragment parsing from a recovery
/// fragment used by `consume_continuation_body`. Element nesting and block
/// nesting both live on `TemplateParser::open_nodes`; `parse_fragment_frame`
/// only needs to know whether a stray `{/...}` should be reported as a
/// stray-at-root error or bubbled out of the recovery frame.
#[derive(Clone, Copy)]
enum FragmentFrame {
    Root,
    RecoveryBlock,
}

impl FragmentFrame {
    fn root() -> Self {
        Self::Root
    }

    fn recovery_block() -> Self {
        Self::RecoveryBlock
    }

    fn is_root(self) -> bool {
        matches!(self, Self::Root)
    }
}

struct AutoClosedTag {
    tag: String,
    reason: String,
}

/// Snapshot of per-element parser context captured on element entry and
/// restored on element exit. Mirrors the implicit "stack frame" vendor Svelte
/// keeps on `parser.stack` for each open element.
struct ElementContext {
    previous_head_context: bool,
    allowed_svelte_self: bool,
    entered_shadowroot_template: bool,
}

/// Pending element waiting for its closing tag. Pushed onto
/// `TemplateParser::open_nodes` (wrapped in `OpenNode::Element`) when a regular
/// element with children opens, popped when the element finishes (explicit
/// close, implicit close, or EOF). Mirrors entries on vendor Svelte's
/// `parser.stack` for `RegularElement` and related node types.
struct OpenElement {
    name: String,
    name_span: Span,
    attributes: Vec<Attribute>,
    attribute_meta: Vec<AttributeMeta>,
    span_start: u32,
    start_tag_end: u32,
    is_head_title: bool,
    context: ElementContext,
}

/// Single entry on the parser's open-node stack. Mirrors entries on vendor
/// Svelte's `parser.stack`. Element variants pair with one fragment frame on
/// `fragments` (the element's children); block variants pair with one
/// fragment frame for the active body / continuation arm. Continuation-
/// bearing blocks like `{#each}` swap fragments on `{:else}` while keeping
/// the same `OpenNode::Block` entry on the stack.
enum OpenNode<'a> {
    Element(OpenElement),
    Block(OpenBlock<'a>),
}

impl<'a> OpenNode<'a> {
    fn as_element(&self) -> Option<&OpenElement> {
        match self {
            Self::Element(e) => Some(e),
            Self::Block(_) => None,
        }
    }
}

/// Pending block waiting for its closing tag. Pushed onto
/// `TemplateParser::open_nodes` (wrapped in `OpenNode::Block`) when a flat
/// block opens, popped when the block finishes via explicit close, mismatched
/// close recovery, continuation-driven implicit close, or EOF.
enum OpenBlock<'a> {
    Key {
        block_start: u32,
        expression: String,
        expression_span: Span,
        body_start: u32,
    },
    Snippet {
        block_start: u32,
        name: String,
        name_span: Span,
        type_params: Option<String>,
        type_params_span: Option<Span>,
        params: String,
        params_span: Option<Span>,
        body_start: u32,
    },
    Each {
        block_start: u32,
        expression: String,
        expression_span: Span,
        context: String,
        context_span: Span,
        index: Option<String>,
        index_span: Option<Span>,
        key: Option<String>,
        key_span: Option<Span>,
        header_span: Span,
        /// Source offset of the start of the *currently active* fragment
        /// (body before `{:else}`, fallback after). Used to construct the
        /// active fragment's `Span` at the next continuation / finalize.
        active_fragment_start: u32,
        /// Captured body fragment, set once the first `{:else}` swaps the
        /// active fragment into fallback mode. While `None`, the dispatch
        /// loop is still appending into the body fragment.
        body: Option<Fragment<'a>>,
    },
    /// One slot in an `{#if} / {:else if} / {:else}` chain.
    ///
    /// All chain entries (the outer `{#if}`, every `{:else if}`, and a
    /// terminating `{:else}`) push a separate `OpenBlock::If` onto the stack
    /// in source order. At `{/if}` (or implicit close) `finalize_if_chain`
    /// walks the chain top-to-bottom, building each element's `IfBlock`
    /// with its `alternate` set to the previously-built deeper node, and
    /// only the outermost (`chained == false`) is appended to the parent
    /// fragment.
    If {
        block_start: u32,
        test: String,
        test_span: Span,
        header_span: Span,
        /// Source offset of the start of this slot's consequent fragment
        /// (the position right after the header's `}`). Also used as the
        /// `span.start` of the AST node for synthetic `{:else}` slots,
        /// matching `parse_else_block_node`'s pre-flatten behavior.
        body_start: u32,
        /// Serialization marker on the AST node — true for `{:else if}`
        /// slots, false for `{#if}` and synthetic `{:else}` slots.
        elseif: bool,
        /// `true` for slots pushed by a `{:else if}` / `{:else}`
        /// continuation, `false` for the outermost `{#if}`. Distinguishes
        /// chain links from the chain root during finalize, and gates the
        /// synthetic-else replacement rule.
        chained: bool,
        /// Captured consequent fragment, set when this slot is "frozen"
        /// because the next chain link is being pushed. While `None`, this
        /// slot is the topmost on the stack and the active fragment on
        /// `fragments` will become its consequent at finalize.
        consequent: Option<Fragment<'a>>,
    },
    Await(OpenAwaitBlock<'a>),
}

/// State for an open `{#await}` block. Tracks the pending / then / catch
/// arms, which arm is currently active (the one whose fragment is being
/// appended into), and the source offset where the active arm's fragment
/// begins (used to construct the arm's `Span` when it's captured by the
/// next continuation or by finalize).
struct OpenAwaitBlock<'a> {
    block_start: u32,
    expression: String,
    expression_span: Span,
    pending: Option<Fragment<'a>>,
    then_arm: AwaitArm<'a>,
    catch_arm: AwaitArm<'a>,
    active: AwaitArmKind,
    active_fragment_start: u32,
}

#[derive(Default)]
struct AwaitArm<'a> {
    fragment: Option<Fragment<'a>>,
    binding: Option<String>,
    binding_span: Option<Span>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AwaitArmKind {
    Pending,
    Then,
    Catch,
}

impl<'a> OpenBlock<'a> {
    fn close_kind(&self) -> BlockClose {
        match self {
            Self::Key { .. } => BlockClose::Key,
            Self::Snippet { .. } => BlockClose::Snippet,
            Self::Each { .. } => BlockClose::Each,
            Self::If { .. } => BlockClose::If,
            Self::Await(_) => BlockClose::Await,
        }
    }
}

impl<'a> TemplateParser<'a> {
    fn new(source: &'a str, allocator: &'a Allocator) -> Self {
        Self {
            source,
            pos: 0,
            allocator,
            errors: Vec::new(),
            fragments: Vec::new(),
            open_nodes: Vec::new(),
            frame_checkpoints: Vec::new(),
            seen_root_meta_tags: Vec::new(),
            in_svelte_head_context: false,
            svelte_self_allowed_depth: 0,
            shadowroot_template_depth: 0,
            reported_unclosed_eof: false,
            last_auto_closed_tag: None,
        }
    }

    /// Push an empty fragment node list onto the parser's fragment stack.
    /// Mirrors vendor Svelte's `parser.fragments.push(create_fragment())`.
    fn enter_fragment(&mut self) {
        self.fragments.push(Vec::new());
    }

    /// Pop the innermost fragment node list off the stack and return it.
    /// Mirrors vendor Svelte's `parser.fragments.pop()`.
    fn exit_fragment(&mut self) -> Vec<TemplateNode<'a>> {
        self.fragments
            .pop()
            .expect("exit_fragment called with no active fragment frame")
    }

    /// Append a node to the innermost fragment. Mirrors vendor Svelte's
    /// `parser.append(node)`.
    fn append(&mut self, node: TemplateNode<'a>) {
        self.fragments
            .last_mut()
            .expect("append called with no active fragment frame")
            .push(node);
    }

    /// `open_nodes.len()` at the start of the current `parse_fragment_frame`
    /// invocation. Entries above this checkpoint were opened inside the
    /// current frame's body; entries at or below belong to outer frames.
    fn current_frame_open_nodes_checkpoint(&self) -> usize {
        *self.frame_checkpoints.last().unwrap_or(&0)
    }

    /// The topmost open *element* scoped to the current frame, or `None` if no
    /// element from this frame is currently open (the topmost entry might be
    /// an open block, or the stack might only hold outer-frame entries). Used
    /// in place of the old `FragmentFrame::Element { name }` variant so block
    /// bodies don't leak the outer element name into placement /
    /// implicit-close logic.
    fn current_frame_parent_element_name(&self) -> Option<&str> {
        let checkpoint = self.current_frame_open_nodes_checkpoint();
        if self.open_nodes.len() > checkpoint {
            self.open_nodes
                .last()
                .and_then(OpenNode::as_element)
                .map(|e| e.name.as_str())
        } else {
            None
        }
    }

    /// True iff we are at the root frame and no node opened in the root
    /// frame's body is currently in scope. Replaces the bare `frame.is_root()`
    /// check now that elements (and, eventually, blocks) live on a
    /// parser-owned stack rather than as recursive frames.
    fn current_frame_is_root_scope(&self, frame: FragmentFrame) -> bool {
        frame.is_root() && self.open_nodes.len() == self.current_frame_open_nodes_checkpoint()
    }

    /// Push per-element context onto the parser before parsing children, and
    /// return a token whose contents must be passed back to
    /// `exit_element_context` after the element finishes. This centralizes the
    /// `in_svelte_head_context`, `svelte_self_allowed_depth`, and
    /// `shadowroot_template_depth` save/restore that previously lived inline
    /// in `parse_element`.
    fn enter_element_context(&mut self, name: &str, attributes: &[Attribute]) -> ElementContext {
        let previous_head_context = self.in_svelte_head_context;
        self.in_svelte_head_context = next_svelte_head_context(name, previous_head_context);

        let allowed_svelte_self = is_regular_component_element_name(name);
        if allowed_svelte_self {
            self.svelte_self_allowed_depth += 1;
        }

        let entered_shadowroot_template = is_shadowroot_template_element(name, attributes);
        if entered_shadowroot_template {
            self.shadowroot_template_depth += 1;
        }

        ElementContext {
            previous_head_context,
            allowed_svelte_self,
            entered_shadowroot_template,
        }
    }

    fn exit_element_context(&mut self, context: ElementContext) {
        if context.entered_shadowroot_template {
            self.shadowroot_template_depth -= 1;
        }
        if context.allowed_svelte_self {
            self.svelte_self_allowed_depth -= 1;
        }
        self.in_svelte_head_context = context.previous_head_context;
    }

    /// Parse the root template fragment.
    fn parse_root_fragment(&mut self) -> Result<Fragment<'a>, OxcDiagnostic> {
        let fragment = self.parse_fragment_frame(FragmentFrame::root())?;
        self.report_post_parse_diagnostics(&fragment.nodes);
        Ok(fragment)
    }

    /// Build an `Element` from the topmost entry on `open_nodes` and append
    /// it to the current parent fragment. Used by both the matched-close-tag
    /// path and every implicit-close path (mismatched close, opening-tag
    /// implicit close, block-close-with-elements-open, EOF). Panics if the
    /// topmost entry is not an open element — callers must check the variant
    /// first.
    fn finalize_top_open_element(
        &mut self,
        end_tag_span: Option<Span>,
        unclosed_at_eof_outer: bool,
    ) -> Result<(), OxcDiagnostic> {
        let pending = match self.open_nodes.pop() {
            Some(OpenNode::Element(e)) => e,
            _ => panic!("finalize_top_open_element called without an open element on top"),
        };
        let children = self.exit_fragment();
        let OpenElement {
            name,
            name_span,
            attributes,
            attribute_meta,
            span_start,
            start_tag_end,
            is_head_title,
            context,
        } = pending;

        self.report_svelte_special_element_content_diagnostics(&name, &children);
        self.report_textarea_content_diagnostics(&name, &attributes, &children);
        if is_head_title {
            self.report_head_title_diagnostics(&attributes, &children);
        }

        self.exit_element_context(context);

        let mut end = self.pos as u32;
        if end as usize >= self.source.len() {
            while end > span_start
                && self.source.as_bytes()[(end - 1) as usize].is_ascii_whitespace()
            {
                end -= 1;
            }
        }

        let element = TemplateNode::Element(Element {
            name,
            name_span,
            attributes,
            attribute_meta,
            children,
            self_closing: false,
            span: Span::new(span_start, end),
            start_tag_end,
            end_tag_span,
            unclosed_at_eof_outer,
        });
        self.append(element);
        Ok(())
    }

    /// Consume a matching `</name>` close tag and finalize the topmost open
    /// element, recording the close-tag span on the resulting `Element`.
    fn finalize_top_open_element_with_close_tag(&mut self) -> Result<(), OxcDiagnostic> {
        debug_assert!(matches!(self.open_nodes.last(), Some(OpenNode::Element(_))));
        debug_assert!(self.looking_at("</"));
        let end_tag_start = self.pos as u32;
        self.eat_until(">");
        self.eat(">")?;
        let end_tag_span = Span::new(end_tag_start, self.pos as u32);
        self.finalize_top_open_element(Some(end_tag_span), false)
    }

    /// Implicit-close the topmost open element without consuming any tokens
    /// from the input.
    fn implicit_close_top_open_element(&mut self) -> Result<(), OxcDiagnostic> {
        self.finalize_top_open_element(None, false)
    }

    /// Implicit-close the topmost open element at EOF, reporting Svelte's
    /// "left open" diagnostic. Only the innermost element produces a visible
    /// diagnostic thanks to `reported_unclosed_eof` deduplication. The
    /// `unclosed_at_eof_outer` flag distinguishes the innermost (vendor's
    /// topmost on its parser stack — gets `end = template.length`) from
    /// outer unclosed entries (vendor leaves at the initial sentinel `-1`).
    fn implicit_close_top_open_element_at_eof(
        &mut self,
        unclosed_at_eof_outer: bool,
    ) -> Result<(), OxcDiagnostic> {
        if let Some(OpenNode::Element(pending)) = self.open_nodes.last() {
            let name = pending.name.clone();
            self.report_unclosed_eof(format!("`<{name}>` was left open"));
        }
        self.finalize_top_open_element(None, unclosed_at_eof_outer)
    }

    /// Build a block AST node from the topmost `OpenBlock` entry on
    /// `open_nodes` and append it to the current parent fragment.
    /// `fragment_end` is the source offset where the *currently active*
    /// body / fallback fragment ends — typically the `{` of the
    /// closing/continuation token, or `self.pos` for EOF. `closed` is true
    /// iff the matching `{/kind}` close tag has just been consumed; for
    /// `OpenBlock::Each` and `OpenBlock::If` an unclosed block also trims
    /// trailing whitespace off the block's outer span, matching the
    /// pre-flatten `parse_each_block` / `parse_if_block` behavior.
    ///
    /// `OpenBlock::If` finalizes through `finalize_if_chain` because the
    /// `{:else if}` chain may have multiple stack entries to pop in one
    /// shot.
    fn finalize_top_open_block(
        &mut self,
        fragment_end: u32,
        closed: bool,
        unclosed_at_eof_outer: bool,
        chain_inner_unclosed_at_eof_outer: bool,
    ) -> Result<(), OxcDiagnostic> {
        if matches!(
            self.open_nodes.last(),
            Some(OpenNode::Block(OpenBlock::If { .. }))
        ) {
            return self.finalize_if_chain(
                fragment_end,
                closed,
                unclosed_at_eof_outer,
                chain_inner_unclosed_at_eof_outer,
            );
        }
        let pending = match self.open_nodes.pop() {
            Some(OpenNode::Block(b)) => b,
            _ => panic!("finalize_top_open_block called without an open block on top"),
        };
        let fragment_nodes = self.exit_fragment();
        let block_node = match pending {
            OpenBlock::Key {
                block_start,
                expression,
                expression_span,
                body_start,
            } => {
                let body = Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(body_start, fragment_end),
                };
                TemplateNode::KeyBlock(KeyBlock {
                    expression,
                    expression_span,
                    body,
                    span: Span::new(block_start, self.pos as u32),
                    unclosed_at_eof_outer,
                })
            }
            OpenBlock::Snippet {
                block_start,
                name,
                name_span,
                type_params,
                type_params_span,
                params,
                params_span,
                body_start,
            } => {
                self.svelte_self_allowed_depth -= 1;
                let body = Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(body_start, fragment_end),
                };
                TemplateNode::SnippetBlock(SnippetBlock {
                    name,
                    name_span,
                    type_params,
                    type_params_span,
                    params,
                    params_span,
                    body,
                    span: Span::new(block_start, self.pos as u32),
                    unclosed_at_eof_outer,
                })
            }
            OpenBlock::Each {
                block_start,
                expression,
                expression_span,
                context,
                context_span,
                index,
                index_span,
                key,
                key_span,
                header_span,
                active_fragment_start,
                body,
            } => {
                self.svelte_self_allowed_depth -= 1;
                let active_fragment = Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(active_fragment_start, fragment_end),
                };
                let (body_fragment, fallback_fragment) = match body {
                    Some(captured_body) => (captured_body, Some(active_fragment)),
                    None => (active_fragment, None),
                };
                let mut span_end = self.pos as u32;
                if !closed {
                    while span_end > block_start
                        && self.source.as_bytes()[(span_end - 1) as usize].is_ascii_whitespace()
                    {
                        span_end -= 1;
                    }
                }
                TemplateNode::EachBlock(EachBlock {
                    expression,
                    expression_span,
                    context,
                    context_span,
                    index,
                    index_span,
                    key,
                    key_span,
                    header_span,
                    body: body_fragment,
                    fallback: fallback_fragment,
                    span: Span::new(block_start, span_end),
                    unclosed_at_eof_outer,
                })
            }
            OpenBlock::If { .. } => {
                // Routed through `finalize_if_chain` above.
                unreachable!("OpenBlock::If is handled by finalize_if_chain");
            }
            OpenBlock::Await(mut a) => {
                let active_fragment = Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(a.active_fragment_start, fragment_end),
                };
                match a.active {
                    AwaitArmKind::Pending => {
                        if a.pending.is_none() {
                            a.pending = Some(active_fragment);
                        }
                    }
                    AwaitArmKind::Then => {
                        if a.then_arm.fragment.is_none() {
                            a.then_arm.fragment = Some(active_fragment);
                        }
                    }
                    AwaitArmKind::Catch => {
                        if a.catch_arm.fragment.is_none() {
                            a.catch_arm.fragment = Some(active_fragment);
                        }
                    }
                }
                TemplateNode::AwaitBlock(AwaitBlock {
                    expression: a.expression,
                    expression_span: a.expression_span,
                    pending: a.pending,
                    then: a.then_arm.fragment,
                    then_binding: a.then_arm.binding,
                    then_binding_span: a.then_arm.binding_span,
                    catch: a.catch_arm.fragment,
                    catch_binding: a.catch_arm.binding,
                    catch_binding_span: a.catch_arm.binding_span,
                    span: Span::new(a.block_start, self.pos as u32),
                    unclosed_at_eof_outer,
                })
            }
        };
        self.append(block_node);
        Ok(())
    }

    /// Walk the `{#if} / {:else if} / {:else}` chain on top of the
    /// `open_nodes` stack from deepest to outermost, building each chain
    /// element's `IfBlock` AST node with `alternate` set to the
    /// previously-built deeper node. Only the outermost (chain root,
    /// `chained == false`) entry is appended to the parent fragment, and
    /// only that one's `svelte_self_allowed_depth` increment is undone.
    ///
    /// `deepest_unclosed_at_eof_outer` stamps `unclosed_at_eof_outer` on the
    /// deepest chain link (vendor's topmost on the parser stack — gets
    /// `end = template.length` on EOF unwinding, so this is `false` only
    /// for the very innermost EOF drain). Every *outer* chain link gets
    /// `chain_inner_unclosed_at_eof_outer` (= `true` for EOF-context
    /// finalize since vendor leaves their `end` at `-1`; `false` for
    /// non-EOF mismatched-close finalize since those build a best-effort
    /// AST that vendor never produces).
    fn finalize_if_chain(
        &mut self,
        fragment_end: u32,
        _closed: bool,
        deepest_unclosed_at_eof_outer: bool,
        chain_inner_unclosed_at_eof_outer: bool,
    ) -> Result<(), OxcDiagnostic> {
        let mut current_alternate: Option<Box<TemplateNode<'a>>> = None;
        let mut is_deepest = true;
        // Vendor's `{:else}` continuation does *not* push onto its parser
        // stack — only `{#if}` and `{:else if}` do. Oxvelte pushes a
        // synthetic-else slot anyway so the dispatch loop has something to
        // hand exit_fragment to. When unwinding at EOF, that synthetic
        // slot is *above* what vendor considers topmost; the actual
        // vendor-topmost is the next chain link below it. Track whether
        // we've assigned the topmost-only `deepest_unclosed_at_eof_outer`
        // flag yet; skip synthetic-else slots so that flag flows to the
        // first non-synthetic chain link.
        let mut deepest_flag_assigned = false;

        loop {
            let pending = match self.open_nodes.pop() {
                Some(OpenNode::Block(OpenBlock::If {
                    block_start,
                    test,
                    test_span,
                    header_span,
                    body_start,
                    elseif,
                    chained,
                    consequent,
                })) => (
                    block_start,
                    test,
                    test_span,
                    header_span,
                    body_start,
                    elseif,
                    chained,
                    consequent,
                ),
                _ => panic!("finalize_if_chain called without an If on top of open_nodes"),
            };
            let (
                block_start,
                test,
                test_span,
                header_span,
                body_start,
                elseif,
                chained,
                stored_consequent,
            ) = pending;

            let consequent = if is_deepest {
                let nodes = self.exit_fragment();
                Fragment {
                    _phantom: PhantomData,
                    nodes,
                    span: Span::new(body_start, fragment_end),
                }
            } else {
                stored_consequent.expect(
                    "ancestor If chain entry must have stored consequent before next chain push",
                )
            };
            let synthetic_else_slot = chained && !elseif;
            let unclosed_at_eof_outer = if synthetic_else_slot {
                // Synthetic-else isn't an `IfBlock` in vendor's AST (it's
                // a Fragment in modern, an ElseBlock in legacy), so the
                // serialized `end` of its IfBlock-shaped node is never
                // emitted by either serializer. The flag value here is
                // therefore unobservable; we use `false` and *don't*
                // consume the deepest-flag slot.
                false
            } else if !deepest_flag_assigned {
                deepest_flag_assigned = true;
                deepest_unclosed_at_eof_outer
            } else {
                chain_inner_unclosed_at_eof_outer
            };
            is_deepest = false;

            // Span starts: synthetic `{:else}` slots use `body_start`
            // (matching `parse_else_block_node`'s `Span::new(content_start,
            // else_end)`); `{#if}` and `{:else if}` slots use `block_start`.
            let synthetic_else = chained && !elseif;
            let span_start = if synthetic_else {
                body_start
            } else {
                block_start
            };
            let span_end = if !chained {
                // Outermost: span ends at `self.pos`, matching pre-flatten
                // `parse_if_block` which always wrote `Span::new(start,
                // self.pos as u32)`. No trailing-whitespace trim — that's
                // an `{#each}`-only quirk.
                self.pos as u32
            } else {
                // Chained slots' AST spans end at `fragment_end` (the `{` of
                // the closing/continuation token), matching the pre-flatten
                // `parse_else_if_block` / `parse_else_block_node` behavior
                // where `self.pos` was at the `{` of `{/if}` when the IfBlock
                // was constructed.
                fragment_end
            };

            let ifblock_node = TemplateNode::IfBlock(IfBlock {
                test,
                test_span,
                header_span,
                elseif,
                consequent,
                alternate: current_alternate,
                span: Span::new(span_start, span_end),
                unclosed_at_eof_outer,
            });

            if !chained {
                self.svelte_self_allowed_depth -= 1;
                self.append(ifblock_node);
                return Ok(());
            }

            current_alternate = Some(Box::new(ifblock_node));
        }
    }

    /// Handle `{:else if X}` arriving while an `OpenBlock::If` is on top of
    /// `open_nodes`. The current active fragment becomes the previous chain
    /// link's consequent (unless that link is a synthetic `{:else}`, in
    /// which case the previous link is dropped and replaced — matching the
    /// pre-flatten `parse_if_alternate` loop's overwrite behavior). A fresh
    /// `OpenBlock::If` with `chained: true, elseif: true` is pushed and a
    /// new fragment is entered for its consequent.
    fn handle_if_else_if_continuation(&mut self) -> Result<(), OxcDiagnostic> {
        debug_assert!(matches!(
            self.open_nodes.last(),
            Some(OpenNode::Block(OpenBlock::If { .. }))
        ));
        let fragment_end = self.pos as u32;
        let fragment_nodes = self.exit_fragment();

        // If the current chain top is a synthetic `{:else}` slot, drop it
        // (and the just-captured fragment_nodes that belonged to it). The
        // new `{:else if}` becomes a sibling of whatever was below.
        if let Some(OpenNode::Block(OpenBlock::If {
            chained: true,
            elseif: false,
            ..
        })) = self.open_nodes.last()
        {
            self.open_nodes.pop();
            // fragment_nodes intentionally dropped.
        } else {
            // Otherwise the captured fragment is the current top's consequent.
            let body_start = match self.open_nodes.last().unwrap() {
                OpenNode::Block(OpenBlock::If { body_start, .. }) => *body_start,
                _ => unreachable!(),
            };
            if let Some(OpenNode::Block(OpenBlock::If { consequent, .. })) =
                self.open_nodes.last_mut()
            {
                *consequent = Some(Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(body_start, fragment_end),
                });
            }
        }

        let block_start = self.pos as u32;
        self.eat_else_if_open()?;
        self.skip_whitespace();
        let test_start = self.pos as u32;
        let test = self.read_expression()?;
        let test_span = Span::new(test_start, self.pos as u32);
        self.eat("}")?;
        let header_span = Span::new(block_start, self.pos as u32);
        let body_start = self.pos as u32;

        self.open_nodes.push(OpenNode::Block(OpenBlock::If {
            block_start,
            test,
            test_span,
            header_span,
            body_start,
            elseif: true,
            chained: true,
            consequent: None,
        }));
        self.enter_fragment();
        Ok(())
    }

    /// Capture the current active fragment of the topmost
    /// `OpenBlock::Await` and stash it in the appropriate arm based on the
    /// active arm kind. Subsequent assignments are dropped silently —
    /// duplicate-arm reporting is the caller's responsibility (matching
    /// pre-flatten `parse_await_continuations`'s `if !duplicate` skip).
    fn capture_active_await_fragment(&mut self) {
        let fragment_end = self.pos as u32;
        let fragment_nodes = self.exit_fragment();
        if let Some(OpenNode::Block(OpenBlock::Await(a))) = self.open_nodes.last_mut() {
            let fragment = Fragment {
                _phantom: PhantomData,
                nodes: fragment_nodes,
                span: Span::new(a.active_fragment_start, fragment_end),
            };
            match a.active {
                AwaitArmKind::Pending => {
                    if a.pending.is_none() {
                        a.pending = Some(fragment);
                    }
                }
                AwaitArmKind::Then => {
                    if a.then_arm.fragment.is_none() {
                        a.then_arm.fragment = Some(fragment);
                    }
                }
                AwaitArmKind::Catch => {
                    if a.catch_arm.fragment.is_none() {
                        a.catch_arm.fragment = Some(fragment);
                    }
                }
            }
        }
    }

    /// Handle `{:then [binding]}` arriving while an `OpenBlock::Await` is on
    /// top. Reports `{:then} cannot appear more than once` for duplicates,
    /// preserves the existing then-arm state in that case, and switches the
    /// active arm to Then. Span override on the Then fragment to start at
    /// `{:then` matches `parse_await_then_clause`'s pre-flatten span fix-up.
    fn handle_await_then_continuation(&mut self) -> Result<(), OxcDiagnostic> {
        self.capture_active_await_fragment();

        let then_tag_start = self.pos as u32;
        self.eat("{:then")?;
        let after_keyword = self.pos;
        self.skip_whitespace();
        let had_whitespace = self.pos > after_keyword;
        let binding_start = self.pos as u32;
        let raw_binding = self.read_block_header();
        let binding_span = span_for_header_part(
            Span::new(binding_start, self.pos as u32),
            raw_binding,
            raw_binding.trim(),
        );
        let binding = raw_binding.trim().to_string();
        if had_whitespace && binding.is_empty() {
            self.report_error(expected_pattern_message());
        }
        self.report_reserved_binding_identifier_diagnostic(&binding);
        self.eat("}")?;

        let already_set = matches!(
            self.open_nodes.last(),
            Some(OpenNode::Block(OpenBlock::Await(a))) if a.then_arm.fragment.is_some()
        );
        if already_set {
            self.report_error("{:then} cannot appear more than once within a block");
        }

        if let Some(OpenNode::Block(OpenBlock::Await(a))) = self.open_nodes.last_mut() {
            if !already_set && !binding.is_empty() {
                a.then_arm.binding = Some(binding);
                a.then_arm.binding_span = Some(binding_span);
            }
            a.active = AwaitArmKind::Then;
            a.active_fragment_start = then_tag_start;
        }
        self.enter_fragment();
        Ok(())
    }

    /// Like `handle_await_then_continuation` but for `{:catch [binding]}`.
    fn handle_await_catch_continuation(&mut self) -> Result<(), OxcDiagnostic> {
        self.capture_active_await_fragment();

        let catch_tag_start = self.pos as u32;
        self.eat("{:catch")?;
        let after_keyword = self.pos;
        self.skip_whitespace();
        let had_whitespace = self.pos > after_keyword;
        let binding_start = self.pos as u32;
        let raw_binding = self.read_block_header();
        let binding_span = span_for_header_part(
            Span::new(binding_start, self.pos as u32),
            raw_binding,
            raw_binding.trim(),
        );
        let binding = raw_binding.trim().to_string();
        if had_whitespace && binding.is_empty() {
            self.report_error(expected_pattern_message());
        }
        self.report_reserved_binding_identifier_diagnostic(&binding);
        self.eat("}")?;

        let already_set = matches!(
            self.open_nodes.last(),
            Some(OpenNode::Block(OpenBlock::Await(a))) if a.catch_arm.fragment.is_some()
        );
        if already_set {
            self.report_error("{:catch} cannot appear more than once within a block");
        }

        if let Some(OpenNode::Block(OpenBlock::Await(a))) = self.open_nodes.last_mut() {
            if !already_set && !binding.is_empty() {
                a.catch_arm.binding = Some(binding);
                a.catch_arm.binding_span = Some(binding_span);
            }
            a.active = AwaitArmKind::Catch;
            a.active_fragment_start = catch_tag_start;
        }
        self.enter_fragment();
        Ok(())
    }

    /// Handle `{:else}` arriving while an `OpenBlock::If` is on top of
    /// `open_nodes`. Like `handle_if_else_if_continuation` but pushes a
    /// synthetic-else slot (`elseif: false, chained: true`) with empty
    /// `test`, matching the pre-flatten `parse_else_block_node` shape.
    fn handle_if_else_continuation(&mut self) -> Result<(), OxcDiagnostic> {
        debug_assert!(matches!(
            self.open_nodes.last(),
            Some(OpenNode::Block(OpenBlock::If { .. }))
        ));
        let fragment_end = self.pos as u32;
        let fragment_nodes = self.exit_fragment();

        if let Some(OpenNode::Block(OpenBlock::If {
            chained: true,
            elseif: false,
            ..
        })) = self.open_nodes.last()
        {
            // Duplicate `{:else}` after a synthetic `{:else}` — replace.
            self.open_nodes.pop();
        } else {
            let body_start = match self.open_nodes.last().unwrap() {
                OpenNode::Block(OpenBlock::If { body_start, .. }) => *body_start,
                _ => unreachable!(),
            };
            if let Some(OpenNode::Block(OpenBlock::If { consequent, .. })) =
                self.open_nodes.last_mut()
            {
                *consequent = Some(Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(body_start, fragment_end),
                });
            }
        }

        let else_start = self.pos as u32;
        self.eat("{:else")?;
        self.skip_whitespace();
        if self.looking_at("}") {
            self.eat("}")?;
        }
        let body_start = self.pos as u32;
        let header_span = Span::new(else_start, body_start);

        self.open_nodes.push(OpenNode::Block(OpenBlock::If {
            block_start: else_start,
            test: String::new(),
            test_span: Span::new(body_start, body_start),
            header_span,
            body_start,
            elseif: false,
            chained: true,
            consequent: None,
        }));
        self.enter_fragment();
        Ok(())
    }

    /// Consume a matching `{/kind}` close tag and finalize the topmost open
    /// block.
    fn finalize_top_open_block_with_close_tag(&mut self) -> Result<(), OxcDiagnostic> {
        debug_assert!(matches!(self.open_nodes.last(), Some(OpenNode::Block(_))));
        debug_assert!(self.looking_at("{/") && !self.looking_at("{/*"));
        let fragment_end = self.pos as u32;
        self.consume_block_close()?;
        self.finalize_top_open_block(fragment_end, true, false, false)
    }

    /// Implicit-close the topmost open block without consuming any tokens.
    /// Used by mismatched-close recovery (caller already reported a more
    /// specific diagnostic) and by non-EOF drain helpers — never sets the
    /// `unclosed_at_eof_outer` flag because vendor errors out on these
    /// inputs rather than producing an AST.
    fn implicit_close_top_open_block(&mut self) -> Result<(), OxcDiagnostic> {
        let fragment_end = self.pos as u32;
        self.finalize_top_open_block(fragment_end, false, false, false)
    }

    /// Implicit-close the topmost open block at EOF, propagating
    /// `unclosed_at_eof_outer` so that the resulting block AST node — and,
    /// for `OpenBlock::If`, every chain link except the deepest — is
    /// stamped with the flag the serializer translates to `end: -1`.
    fn implicit_close_top_open_block_at_eof(
        &mut self,
        unclosed_at_eof_outer: bool,
    ) -> Result<(), OxcDiagnostic> {
        let fragment_end = self.pos as u32;
        self.finalize_top_open_block(fragment_end, false, unclosed_at_eof_outer, true)
    }

    /// Swap the active fragment of an `OpenBlock::Each` from body to fallback
    /// on `{:else}`. Caller must have already verified the topmost entry is
    /// `OpenBlock::Each` (panics otherwise). For a duplicate `{:else}` (body
    /// already captured) the previous fallback is dropped — matching the
    /// pre-flatten `parse_each_block` continuation loop's override behavior.
    fn handle_each_else_continuation(&mut self) -> Result<(), OxcDiagnostic> {
        debug_assert!(matches!(
            self.open_nodes.last(),
            Some(OpenNode::Block(OpenBlock::Each { .. }))
        ));
        let fragment_end = self.pos as u32;
        let fragment_nodes = self.exit_fragment();

        self.eat("{:else")?;
        self.skip_whitespace();
        if self.looking_at("}") {
            self.eat("}")?;
        }
        let new_fragment_start = self.pos as u32;

        if let Some(OpenNode::Block(OpenBlock::Each {
            active_fragment_start,
            body,
            ..
        })) = self.open_nodes.last_mut()
        {
            if body.is_none() {
                *body = Some(Fragment {
                    _phantom: PhantomData,
                    nodes: fragment_nodes,
                    span: Span::new(*active_fragment_start, fragment_end),
                });
            }
            *active_fragment_start = new_fragment_start;
        }

        self.enter_fragment();
        Ok(())
    }

    /// Drain helper used by `parse_fragment_frame` when a non-element exit
    /// token is bubbling through and entries pushed in this frame must be
    /// closed first. Reports the block-specific "Unclosed kind block"
    /// diagnostic (matching the pre-flatten `recover_block_close` behavior)
    /// when the topmost entry is an open block; element drains stay silent
    /// like before.
    fn drain_implicit_close_top(&mut self) -> Result<(), OxcDiagnostic> {
        match self.open_nodes.last() {
            Some(OpenNode::Element(_)) => self.implicit_close_top_open_element(),
            Some(OpenNode::Block(block)) => {
                let close = block.close_kind();
                self.report_error(close.unclosed_message());
                self.implicit_close_top_open_block()
            }
            None => Ok(()),
        }
    }

    /// Drain helper at EOF. Mirrors `recover_block_close`'s EOF path
    /// ("Block was left open") for blocks while preserving the element-level
    /// "left open" diagnostic. Both messages funnel through
    /// `report_unclosed_eof` so only the innermost is visible. The
    /// per-iteration `is_innermost` snapshot (taken before any diagnostic
    /// flips `reported_unclosed_eof`) drives the `unclosed_at_eof_outer`
    /// flag on the resulting AST node: vendor's parser sets the topmost
    /// open node's `end` to `template.length` (matching our `self.pos`
    /// behavior) and leaves every other in-stack node's `end` at the
    /// initial `-1` sentinel.
    fn drain_implicit_close_top_at_eof(&mut self) -> Result<(), OxcDiagnostic> {
        let is_innermost = !self.reported_unclosed_eof;
        match self.open_nodes.last() {
            Some(OpenNode::Element(_)) => {
                self.implicit_close_top_open_element_at_eof(!is_innermost)
            }
            Some(OpenNode::Block(_)) => {
                self.report_unclosed_eof("Block was left open");
                self.implicit_close_top_open_block_at_eof(!is_innermost)
            }
            None => Ok(()),
        }
    }

    fn parse_fragment_frame(
        &mut self,
        frame: FragmentFrame,
    ) -> Result<Fragment<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        // Remember the open-node stack depth so block/element closes that
        // belong to outer frames don't accidentally finalize inner entries,
        // and so placement diagnostics see the right parent name.
        let open_nodes_checkpoint = self.open_nodes.len();
        self.frame_checkpoints.push(open_nodes_checkpoint);
        self.enter_fragment();

        while self.pos < self.source.len() {
            match self.parse_fragment_step(frame)? {
                FragmentStep::Node(node) => self.append(node),
                FragmentStep::Exit => {
                    // If a block close / continuation / stray close is bubbling
                    // up while entries are still open inside this frame's
                    // scope, implicit-close them first and re-poll the same
                    // token. Eventually the open-node stack drops back to
                    // the checkpoint and the exit propagates to the caller.
                    if self.open_nodes.len() > open_nodes_checkpoint {
                        self.drain_implicit_close_top()?;
                        continue;
                    }
                    break;
                }
                FragmentStep::Continue => continue,
            }
        }

        // EOF: any entries opened inside this frame and never closed must be
        // finalized. The innermost entry gets Svelte's "left open" diagnostic
        // via `reported_unclosed_eof` deduplication.
        while self.open_nodes.len() > open_nodes_checkpoint {
            self.drain_implicit_close_top_at_eof()?;
        }

        let nodes = self.exit_fragment();
        self.frame_checkpoints.pop();
        Ok(Fragment {
            _phantom: PhantomData,
            nodes,
            span: Span::new(start, self.pos as u32),
        })
    }

    fn parse_fragment_step(
        &mut self,
        frame: FragmentFrame,
    ) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        // The open-element stack now carries element parents. The frame
        // argument only distinguishes root vs block; element nesting is
        // tracked via `open_nodes` and the per-frame checkpoint.
        let is_root = self.current_frame_is_root_scope(frame);
        let parent_open_name = self
            .current_frame_parent_element_name()
            .map(|s| s.to_string());

        // Skip over <script> and <style> blocks at top level only.
        if is_root && (self.looking_at_start_tag("script") || self.looking_at_start_tag("style")) {
            self.skip_block()?;
            return Ok(FragmentStep::Continue);
        }

        // Check for implicit closing (e.g., <li> closes previous <li>).
        if let Some(parent_name) = parent_open_name.as_deref() {
            if self.looking_at("<") && !self.looking_at("</") && !self.looking_at("<!") {
                let peek_name = self.peek_tag_name();
                if should_implicitly_close(parent_name, &peek_name) {
                    self.last_auto_closed_tag = Some(AutoClosedTag {
                        tag: parent_name.to_string(),
                        reason: peek_name.clone(),
                    });
                    self.implicit_close_top_open_element()?;
                    return Ok(FragmentStep::Continue);
                }
            }
        }

        if self.looking_at("</") {
            let close_name = self.peek_close_tag_name();
            let checkpoint = self.current_frame_open_nodes_checkpoint();

            // If the topmost entry in this frame is a flat block, that block
            // needs to be auto-closed first (mirroring vendor's stack
            // unwinding) before the element-close logic can find an element
            // parent below it.
            if self.open_nodes.len() > checkpoint
                && matches!(self.open_nodes.last(), Some(OpenNode::Block(_)))
            {
                self.drain_implicit_close_top()?;
                return Ok(FragmentStep::Continue);
            }

            if let Some(parent_name) = parent_open_name.as_deref() {
                if !close_name.is_empty() && !close_name.eq_ignore_ascii_case(parent_name) {
                    // Mismatched close — implicit-close the inner element and
                    // re-poll the same token so an outer match can claim it.
                    self.implicit_close_top_open_element()?;
                    return Ok(FragmentStep::Continue);
                }
                // Matches the innermost open element — finalize it.
                self.finalize_top_open_element_with_close_tag()?;
                return Ok(FragmentStep::Continue);
            }
            // No element is currently open.
            if frame.is_root() {
                if let Some(reason) = self.take_auto_closed_reason(&close_name) {
                    self.report_error(format!(
                        "`</{close_name}>` attempted to close element that was already automatically closed by `<{reason}>` (cannot nest `<{reason}>` inside `<{close_name}>`)"
                    ));
                } else if is_void_element(&close_name) {
                    self.report_error("Void elements cannot have children or closing tags");
                } else {
                    self.report_error(format!(
                        "`</{close_name}>` attempted to close an element that was not open"
                    ));
                }
                self.consume_html_close()?;
                Ok(FragmentStep::Continue)
            } else {
                // Inside a recovery frame with no open elements — bubble up
                // so the recovery's caller can keep parsing past the close.
                let _ = close_name;
                Ok(FragmentStep::Exit)
            }
        } else if self.looking_at("{/") && !self.looking_at("{/*") {
            let found = self.peek_block_close_name();
            let checkpoint = self.current_frame_open_nodes_checkpoint();

            // If something opened by *this* frame is still on the open-node
            // stack, give it a chance to claim or be auto-closed by this
            // close tag before bubbling up to a recursive block parser.
            if self.open_nodes.len() > checkpoint {
                match self.open_nodes.last().unwrap() {
                    OpenNode::Element(_) => {
                        // Element on top — element close-handling already
                        // implicit-closed mismatched element parents above; if
                        // we got here with an Element on top it's a stray
                        // inner element. Implicit-close it and re-poll.
                        self.implicit_close_top_open_element()?;
                        return Ok(FragmentStep::Continue);
                    }
                    OpenNode::Block(block) => {
                        let kind = block.close_kind();
                        if kind.tag() == found {
                            // Matching close — finalize the block.
                            self.finalize_top_open_block_with_close_tag()?;
                            return Ok(FragmentStep::Continue);
                        }
                        // Mismatched close for a flat block. Mirror the
                        // pre-flatten `finish_block` BlockClose-mismatch path:
                        // report "Expected token kind" and finalize without
                        // consuming the close, so an outer recursive block
                        // can still claim it.
                        self.report_error(format!("Expected token {}", kind.tag()));
                        self.implicit_close_top_open_block()?;
                        return Ok(FragmentStep::Continue);
                    }
                }
            }

            if is_root {
                self.report_error("Unexpected block closing tag");
                self.consume_block_close()?;
                Ok(FragmentStep::Continue)
            } else {
                // Block closing tag inside a recovery frame — bubble up so
                // the recovery's caller sees it.
                let _ = found;
                Ok(FragmentStep::Exit)
            }
        } else if self.looking_at("{:") {
            let continuation = self
                .peek_block_continuation()
                .unwrap_or(BlockContinuation::Other);

            // If the topmost flat block in this frame is `{#each}` or `{#if}`,
            // handle the continuation inline. Other blocks (Key, Snippet) and
            // elements fall through to bubble Exit; the surrounding
            // `parse_fragment_frame` drains those entries via
            // `drain_implicit_close_top`, which reports the block-specific
            // "Unclosed kind block" diagnostic that matches the pre-flatten
            // `recover_block_close` else-branch.
            let checkpoint = self.current_frame_open_nodes_checkpoint();
            if self.open_nodes.len() > checkpoint {
                match self.open_nodes.last().unwrap() {
                    OpenNode::Block(OpenBlock::Each { .. }) => {
                        if continuation == BlockContinuation::Else {
                            self.handle_each_else_continuation()?;
                            return Ok(FragmentStep::Continue);
                        }
                        self.report_error("Expected token {:else}");
                        self.consume_continuation_body()?;
                        return Ok(FragmentStep::Continue);
                    }
                    OpenNode::Block(OpenBlock::If { .. }) => match continuation {
                        BlockContinuation::Else => {
                            self.handle_if_else_continuation()?;
                            return Ok(FragmentStep::Continue);
                        }
                        BlockContinuation::ElseIf => {
                            self.handle_if_else_if_continuation()?;
                            return Ok(FragmentStep::Continue);
                        }
                        BlockContinuation::InvalidElseIf => {
                            self.report_error("'elseif' should be 'else if'");
                            self.consume_continuation_body()?;
                            return Ok(FragmentStep::Continue);
                        }
                        BlockContinuation::Then
                        | BlockContinuation::Catch
                        | BlockContinuation::Other => {
                            self.report_error("Expected token {:else} or {:else if}");
                            self.consume_continuation_body()?;
                            return Ok(FragmentStep::Continue);
                        }
                    },
                    OpenNode::Block(OpenBlock::Await(_)) => match continuation {
                        BlockContinuation::Then => {
                            self.handle_await_then_continuation()?;
                            return Ok(FragmentStep::Continue);
                        }
                        BlockContinuation::Catch => {
                            self.handle_await_catch_continuation()?;
                            return Ok(FragmentStep::Continue);
                        }
                        BlockContinuation::Else
                        | BlockContinuation::ElseIf
                        | BlockContinuation::InvalidElseIf
                        | BlockContinuation::Other => {
                            self.report_error("Expected token {:then ...} or {:catch ...}");
                            self.consume_continuation_body()?;
                            return Ok(FragmentStep::Continue);
                        }
                    },
                    OpenNode::Block(OpenBlock::Key { .. } | OpenBlock::Snippet { .. })
                    | OpenNode::Element(_) => {
                        // Bubble — drain handler closes element/Key/Snippet.
                    }
                }
            }

            // Block continuation tag the topmost flat block didn't claim —
            // bubble up so a recovery frame (or root) can move past it.
            let _ = continuation;
            Ok(FragmentStep::Exit)
        } else if self.looking_at("<!--") {
            self.parse_comment().map(FragmentStep::Node)
        } else if self.looking_at_svelte_keyword_missing_whitespace(
            "{#",
            &["if", "each", "await", "key", "snippet"],
        ) || self
            .looking_at_svelte_keyword_missing_whitespace("{@", &["html", "render", "const"])
        {
            self.report_error("Expected whitespace");
            Ok(FragmentStep::Node(self.parse_mustache_with_recovery()))
        } else if self.looking_at_block_start("if") {
            self.parse_if_block()
        } else if self.looking_at_block_start("each") {
            self.parse_each_block()
        } else if self.looking_at_block_start("await") {
            self.parse_await_block()
        } else if self.looking_at_block_start("key") {
            self.parse_key_block()
        } else if self.looking_at_block_start("snippet") {
            self.parse_snippet_block()
        } else if self.looking_at_special_tag("html") {
            self.parse_raw_mustache().map(FragmentStep::Node)
        } else if self.looking_at_special_tag("debug") {
            self.parse_debug_tag().map(FragmentStep::Node)
        } else if self.looking_at_special_tag("const") {
            self.parse_const_tag().map(FragmentStep::Node)
        } else if self.looking_at_special_tag("render") {
            self.parse_render_tag().map(FragmentStep::Node)
        } else if self.looking_at("{@") {
            self.report_error("Expected 'html', 'render', 'attach', 'const', or 'debug'");
            Ok(FragmentStep::Node(self.parse_mustache_with_recovery()))
        } else if self.looking_at("{") {
            Ok(FragmentStep::Node(self.parse_mustache_with_recovery()))
        } else if self.looking_at("<") {
            Ok(self.parse_element_with_recovery(frame))
        } else {
            self.parse_text().map(FragmentStep::Node)
        }
    }

    // ─── Helpers ───────────────────────────────────────────────────────

    #[inline]
    fn looking_at(&self, prefix: &str) -> bool {
        self.source[self.pos..].starts_with(prefix)
    }

    #[inline]
    fn looking_at_block_start(&self, keyword: &str) -> bool {
        self.looking_at_svelte_keyword("{#", keyword)
    }

    #[inline]
    fn looking_at_special_tag(&self, keyword: &str) -> bool {
        self.looking_at_svelte_keyword("{@", keyword)
    }

    #[inline]
    fn looking_at_continuation(&self, keyword: &str) -> bool {
        self.looking_at_svelte_keyword("{:", keyword)
    }

    fn looking_at_svelte_keyword(&self, prefix: &str, keyword: &str) -> bool {
        self.looking_at(prefix)
            && self.source[self.pos + prefix.len()..].starts_with(keyword)
            && scanner::is_svelte_keyword_boundary(
                self.source,
                self.pos + prefix.len() + keyword.len(),
            )
    }

    fn looking_at_svelte_keyword_missing_whitespace(
        &self,
        prefix: &str,
        keywords: &[&str],
    ) -> bool {
        if !self.looking_at(prefix) {
            return false;
        }

        keywords.iter().any(|keyword| {
            let after_keyword = self.pos + prefix.len() + keyword.len();
            self.source[self.pos + prefix.len()..].starts_with(keyword)
                && self
                    .source
                    .as_bytes()
                    .get(after_keyword)
                    .is_some_and(|ch| !ch.is_ascii_whitespace())
        })
    }

    fn looking_at_else_if(&self) -> bool {
        if !self.looking_at_continuation("else") {
            return false;
        }
        let mut pos = self.pos + "{:else".len();
        let bytes = self.source.as_bytes();
        while pos < self.source.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        self.source[pos..].starts_with("if")
            && scanner::is_svelte_keyword_boundary(self.source, pos + 2)
    }

    fn looking_at_invalid_elseif(&self) -> bool {
        self.looking_at("{:elseif")
            && scanner::is_svelte_keyword_boundary(self.source, self.pos + "{:elseif".len())
    }

    fn peek_block_continuation(&self) -> Option<BlockContinuation> {
        if !self.looking_at("{:") {
            return None;
        }

        if self.looking_at_invalid_elseif() {
            Some(BlockContinuation::InvalidElseIf)
        } else if self.looking_at_else_if() {
            Some(BlockContinuation::ElseIf)
        } else if self.looking_at_continuation("else") && self.is_else_closing() {
            Some(BlockContinuation::Else)
        } else if self.looking_at_continuation("then") {
            Some(BlockContinuation::Then)
        } else if self.looking_at_continuation("catch") {
            Some(BlockContinuation::Catch)
        } else {
            Some(BlockContinuation::Other)
        }
    }

    fn peek_block_close_name(&self) -> String {
        if !self.looking_at("{/") || self.looking_at("{/*") {
            return String::new();
        }

        let mut pos = self.pos + 2;
        let bytes = self.source.as_bytes();
        while pos < self.source.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }

        let name_start = pos;
        while pos < self.source.len() {
            let byte = bytes[pos];
            if byte.is_ascii_whitespace() || byte == b'}' {
                break;
            }
            pos += 1;
        }

        self.source[name_start..pos].to_string()
    }

    #[inline]
    fn looking_at_start_tag(&self, tag_name: &str) -> bool {
        let prefix = format!("<{}", tag_name);
        scanner::starts_with_ascii_case_insensitive(self.remaining(), &prefix)
            && scanner::is_tag_name_boundary(self.source, self.pos + prefix.len())
    }

    #[inline]
    fn remaining(&self) -> &'a str {
        &self.source[self.pos..]
    }

    fn eat(&mut self, expected: &str) -> Result<(), OxcDiagnostic> {
        if self.looking_at(expected) {
            self.pos += expected.len();
            Ok(())
        } else {
            Err(OxcDiagnostic::error(format!(
                "Expected '{}' at position {}",
                expected, self.pos
            )))
        }
    }

    fn eat_until(&mut self, delimiter: &str) -> &'a str {
        if let Some(idx) = self.remaining().find(delimiter) {
            let text = &self.source[self.pos..self.pos + idx];
            self.pos += idx;
            text
        } else {
            let text = self.remaining();
            self.pos = self.source.len();
            text
        }
    }

    fn eat_until_any(&mut self, delimiters: &[&str]) -> &'a str {
        let mut earliest = self.source.len();
        for delim in delimiters {
            if let Some(idx) = self.remaining().find(delim) {
                earliest = earliest.min(self.pos + idx);
            }
        }
        let text = &self.source[self.pos..earliest];
        self.pos = earliest;
        text
    }

    fn report_error(&mut self, message: impl Into<String>) {
        self.errors.push(OxcDiagnostic::error(message.into()));
    }

    fn report_reserved_binding_identifier_diagnostic(&mut self, identifier: &str) {
        let identifier = identifier.trim();
        if oxc::syntax::keyword::is_reserved_keyword(identifier) {
            self.report_error(unexpected_reserved_word_message(identifier));
        }
    }

    fn report_unclosed_eof(&mut self, message: impl Into<String>) {
        if self.reported_unclosed_eof {
            return;
        }
        self.reported_unclosed_eof = true;
        self.report_error(message);
    }

    fn take_auto_closed_reason(&mut self, close_name: &str) -> Option<String> {
        if self
            .last_auto_closed_tag
            .as_ref()
            .is_some_and(|tag| tag.tag.eq_ignore_ascii_case(close_name))
        {
            return self.last_auto_closed_tag.take().map(|tag| tag.reason);
        }
        None
    }

    fn consume_continuation_body(&mut self) -> Result<(), OxcDiagnostic> {
        self.eat("{:")?;
        self.eat_until("}");
        if self.looking_at("}") {
            self.eat("}")?;
        }
        // Parse the malformed continuation's body in a recovery frame; its
        // nodes are discarded along with the returned `Fragment`.
        let _ = self.parse_fragment_frame(FragmentFrame::recovery_block())?;
        Ok(())
    }

    fn consume_block_close(&mut self) -> Result<(), OxcDiagnostic> {
        self.eat("{/")?;
        self.eat_until("}");
        if self.looking_at("}") {
            self.eat("}")?;
        }
        Ok(())
    }

    fn consume_html_close(&mut self) -> Result<(), OxcDiagnostic> {
        self.eat("</")?;
        self.eat_until(">");
        if self.looking_at(">") {
            self.eat(">")?;
        }
        Ok(())
    }

    fn read_block_header(&mut self) -> &'a str {
        let start = self.pos;
        self.pos = scanner::find_expression_end(self.source, self.pos);
        &self.source[start..self.pos]
    }

    /// Read a quoted attribute value, properly skipping over `{...}` expressions
    /// that may contain the same quote character inside JS strings.
    fn eat_quoted_attr_value(&mut self, quote: u8) -> String {
        let start = self.pos;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == quote {
                // Found the closing attribute quote
                return self.source[start..self.pos].to_string();
            }
            if ch == b'{' {
                self.pos += 1;
                let _ = self.read_expression();
                if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b'}' {
                    self.pos += 1;
                }
                continue;
            }
            self.pos += 1;
        }
        self.source[start..self.pos].to_string()
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.source.len() && self.source.as_bytes()[self.pos].is_ascii_whitespace()
        {
            self.pos += 1;
        }
    }

    /// Parse children of raw text elements.
    /// HTML tags are treated as text. Textarea parses mustache expressions,
    /// while nested script/style keep braces as raw text like Svelte.
    fn parse_raw_text_children(
        &mut self,
        tag_name: &str,
        parse_mustaches: bool,
    ) -> Result<(Vec<TemplateNode<'a>>, Option<Span>), OxcDiagnostic> {
        let close_prefix = format!("</{}", tag_name);
        let mut nodes = Vec::new();
        let mut end_tag_span = None;

        while self.pos < self.source.len() {
            // Check for closing tag — </tagname followed by whitespace or >
            if scanner::starts_with_ascii_case_insensitive(self.remaining(), &close_prefix) {
                let after_prefix = &self.source[self.pos + close_prefix.len()..];
                let next_ch = after_prefix.chars().next();
                if next_ch == Some('>') || next_ch.map(|c| c.is_ascii_whitespace()).unwrap_or(true)
                {
                    // Valid closing tag — eat to >
                    let end_tag_start = self.pos as u32;
                    self.eat_until(">");
                    if self.looking_at(">") {
                        self.eat(">")?;
                    }
                    end_tag_span = Some(Span::new(end_tag_start, self.pos as u32));
                    break;
                }
                // Not a valid closing tag (e.g., </textaread) — treat as text
            }

            if tag_name.eq_ignore_ascii_case("textarea")
                && (self.looking_at("{#") || self.looking_at("{@"))
            {
                self.report_invalid_textarea_tag_placement();
            }

            if parse_mustaches && self.looking_at("{") && !self.looking_at("{{") {
                // Mustache expression
                nodes.push(self.parse_mustache()?);
            } else {
                // Raw text until next parseable { or closing tag prefix
                let text_start = self.pos as u32;
                while self.pos < self.source.len() {
                    if parse_mustaches && self.looking_at("{") {
                        break;
                    }
                    if scanner::starts_with_ascii_case_insensitive(self.remaining(), &close_prefix)
                    {
                        let after_prefix = &self.source[self.pos + close_prefix.len()..];
                        let next_ch = after_prefix.chars().next();
                        if next_ch == Some('>')
                            || next_ch.map(|c| c.is_ascii_whitespace()).unwrap_or(true)
                        {
                            break;
                        }
                    }
                    // Advance by the full UTF-8 character length to avoid
                    // landing inside a multi-byte character.
                    let ch_len = utf8_char_len(self.source.as_bytes()[self.pos]);
                    self.pos += ch_len;
                }
                let text = &self.source[text_start as usize..self.pos];
                if !text.is_empty() {
                    nodes.push(TemplateNode::Text(Text {
                        data: text.to_string(),
                        span: Span::new(text_start, self.pos as u32),
                    }));
                }
            }
        }

        Ok((nodes, end_tag_span))
    }

    fn report_invalid_textarea_tag_placement(&mut self) {
        let marker = self.source.as_bytes().get(self.pos + 1).copied();
        let Some(marker) = marker else {
            return;
        };

        let name_start = self.pos + 2;
        let mut name_end = name_start;
        while name_end < self.source.len() && self.source.as_bytes()[name_end].is_ascii_alphabetic()
        {
            name_end += 1;
        }
        let name = &self.source[name_start..name_end];

        if marker == b'#' {
            self.report_error(format!("{{#{name} ...}} block cannot be inside <textarea>"));
        } else if marker == b'@' {
            self.report_error(format!("{{@{name} ...}} tag cannot be inside <textarea>"));
        }
    }

    /// Check if we're at `{:else` followed by whitespace then `}` (not `{:else if`).
    fn is_else_closing(&self) -> bool {
        if !self.looking_at_continuation("else") {
            return false;
        }
        if self.looking_at_else_if() {
            return false;
        }
        if self.looking_at("{:else}") {
            return true;
        }
        let after = &self.source[self.pos + 6..];
        after.trim_start().starts_with('}')
    }

    /// Peek at the closing tag name (e.g., "</div>" → "div") without advancing.
    fn peek_close_tag_name(&self) -> String {
        let remaining = self.remaining();
        if !remaining.starts_with("</") {
            return String::new();
        }
        let after = &remaining[2..];
        let end = scanner::read_tag_name_end(after, 0);
        after[..end].to_string()
    }

    /// Peek at the next tag name without advancing the parser position.
    fn peek_tag_name(&self) -> String {
        let remaining = self.remaining();
        if !remaining.starts_with('<') {
            return String::new();
        }
        let after_lt = &remaining[1..];
        let end = scanner::read_tag_name_end(after_lt, 0);
        after_lt[..end].to_string()
    }

    /// Skip a `<script>` or `<style>` block entirely.
    fn skip_block(&mut self) -> Result<(), OxcDiagnostic> {
        let is_script = self.looking_at_start_tag("script");
        let close_prefix = if is_script { "</script" } else { "</style" };
        let close_tag_exact = if is_script { "</script>" } else { "</style>" };

        let Some(open_end) = scanner::find_tag_end(self.source, self.pos) else {
            self.pos = self.source.len();
            return Ok(());
        };
        self.pos = (open_end + 1).min(self.source.len());

        // Try exact match first, then prefix with whitespace
        loop {
            if let Some(idx) = scanner::find_ascii_case_insensitive(self.remaining(), close_prefix)
            {
                self.pos += idx;
            } else {
                self.pos = self.source.len();
                break;
            }
            if scanner::starts_with_ascii_case_insensitive(self.remaining(), close_tag_exact) {
                self.pos += close_tag_exact.len();
                break;
            }
            if scanner::starts_with_ascii_case_insensitive(self.remaining(), close_prefix) {
                let after = &self.source[self.pos + close_prefix.len()..];
                if after.trim_start().starts_with('>') {
                    // Skip to the >
                    self.pos += close_prefix.len();
                    while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != b'>' {
                        self.pos += 1;
                    }
                    if self.pos < self.source.len() {
                        self.pos += 1; // skip >
                    }
                    break;
                }
                // Not a valid close tag, skip past this occurrence
                self.pos += close_prefix.len();
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Read a balanced `{...}` expression, handling nested braces.
    fn read_expression(&mut self) -> Result<String, OxcDiagnostic> {
        let mut depth = 0i32;
        let start = self.pos;
        let bytes = self.source.as_bytes();

        while self.pos < self.source.len() {
            match bytes[self.pos] {
                b'{' => depth += 1,
                b'}' => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                b'\'' | b'"' | b'`' => {
                    self.skip_string_literal(bytes[self.pos])?;
                    continue;
                }
                b'/' if self.pos + 1 < self.source.len() => match bytes[self.pos + 1] {
                    b'/' => {
                        self.skip_line_comment();
                        continue;
                    }
                    b'*' => {
                        self.skip_block_comment();
                        continue;
                    }
                    _ if self.slash_starts_regex(start) => {
                        self.skip_regex_literal();
                        continue;
                    }
                    _ => {}
                },
                _ => {}
            }
            self.pos += 1;
        }

        Ok(self.source[start..self.pos].to_string())
    }

    fn skip_line_comment(&mut self) {
        self.pos += 2;
        while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != b'\n' {
            self.pos += 1;
        }
    }

    fn skip_block_comment(&mut self) {
        self.pos += 2;
        while self.pos + 1 < self.source.len() {
            if self.source.as_bytes()[self.pos] == b'*'
                && self.source.as_bytes()[self.pos + 1] == b'/'
            {
                self.pos += 2;
                return;
            }
            self.pos += 1;
        }
        self.pos = self.source.len();
    }

    fn skip_regex_literal(&mut self) {
        self.pos += 1;
        let mut in_char_class = false;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == b'\\' {
                self.pos = (self.pos + 2).min(self.source.len());
                continue;
            }
            if in_char_class {
                if ch == b']' {
                    in_char_class = false;
                }
                self.pos += 1;
                continue;
            }
            match ch {
                b'[' => {
                    in_char_class = true;
                    self.pos += 1;
                }
                b'/' => {
                    self.pos += 1;
                    while self.pos < self.source.len()
                        && self.source.as_bytes()[self.pos].is_ascii_alphabetic()
                    {
                        self.pos += 1;
                    }
                    return;
                }
                b'\n' | b'\r' => return,
                _ => self.pos += 1,
            }
        }
    }

    fn slash_starts_regex(&self, expr_start: usize) -> bool {
        let bytes = self.source.as_bytes();
        let mut i = self.pos;
        while i > expr_start {
            i -= 1;
            let ch = bytes[i];
            if ch.is_ascii_whitespace() {
                continue;
            }
            return matches!(
                ch,
                b'(' | b'['
                    | b'{'
                    | b'='
                    | b':'
                    | b','
                    | b';'
                    | b'!'
                    | b'?'
                    | b'&'
                    | b'|'
                    | b'+'
                    | b'-'
                    | b'*'
                    | b'%'
                    | b'^'
                    | b'~'
                    | b'<'
                    | b'>'
            );
        }
        true
    }

    /// Skip a string literal (handles escaped quotes).
    fn skip_string_literal(&mut self, quote: u8) -> Result<(), OxcDiagnostic> {
        self.pos += 1; // skip opening quote
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == b'\\' {
                self.pos += 2; // skip escaped char + next
                continue;
            }
            // Handle template literal ${...} expressions
            if quote == b'`'
                && ch == b'$'
                && self.pos + 1 < self.source.len()
                && self.source.as_bytes()[self.pos + 1] == b'{'
            {
                self.pos += 2; // skip ${
                let expr_start = self.pos;
                let mut expr_depth = 1i32;
                while self.pos < self.source.len() && expr_depth > 0 {
                    let inner = self.source.as_bytes()[self.pos];
                    match inner {
                        b'{' => expr_depth += 1,
                        b'}' => {
                            expr_depth -= 1;
                            if expr_depth == 0 {
                                self.pos += 1; // skip closing }
                                break;
                            }
                        }
                        b'\'' | b'"' | b'`' => {
                            self.skip_string_literal(inner)?;
                            continue; // skip_string_literal already advanced pos
                        }
                        b'/' if self.pos + 1 < self.source.len() => {
                            match self.source.as_bytes()[self.pos + 1] {
                                b'/' => {
                                    self.skip_line_comment();
                                    continue;
                                }
                                b'*' => {
                                    self.skip_block_comment();
                                    continue;
                                }
                                _ if self.slash_starts_regex(expr_start) => {
                                    self.skip_regex_literal();
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        b'\\' => {
                            self.pos = (self.pos + 2).min(self.source.len());
                            continue;
                        }
                        _ => {}
                    }
                    self.pos += 1;
                }
                continue; // continue reading the template literal
            }
            if ch == quote {
                self.pos += 1; // skip closing quote
                return Ok(());
            }
            self.pos += 1;
        }
        Err(OxcDiagnostic::error("Unterminated string literal"))
    }

    // ─── Node parsers ──────────────────────────────────────────────────

    fn parse_mustache_with_recovery(&mut self) -> TemplateNode<'a> {
        let recovery_start = self.pos;
        match self.parse_mustache() {
            Ok(node) => node,
            Err(_) => {
                // Restore pos and emit a single "{" text node so we make forward progress.
                self.pos = recovery_start + 1;
                TemplateNode::Text(Text {
                    data: "{".to_string(),
                    span: Span::new(recovery_start as u32, self.pos as u32),
                })
            }
        }
    }

    fn parse_element_with_recovery(&mut self, frame: FragmentFrame) -> FragmentStep<'a> {
        match self.parse_element(frame) {
            Ok(step) => step,
            Err(_) => {
                let recovery_start = self.pos as u32;
                while self.pos < self.source.len() {
                    let ch = self.source.as_bytes()[self.pos];
                    if ch == b'>' {
                        self.pos += 1;
                        break;
                    }
                    if ch == b'\n' {
                        break;
                    }
                    self.pos += 1;
                }

                if self.pos as u32 > recovery_start {
                    FragmentStep::Node(TemplateNode::Text(Text {
                        data: self.source[recovery_start as usize..self.pos].to_string(),
                        span: Span::new(recovery_start, self.pos as u32),
                    }))
                } else {
                    FragmentStep::Continue
                }
            }
        }
    }

    fn parse_text(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        let data = self.eat_until_any(&["<", "{", "<!--"]);
        Ok(TemplateNode::Text(Text {
            data: data.to_string(),
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_comment(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("<!--")?;
        let data = self.eat_until("-->");
        self.eat("-->")?;
        Ok(TemplateNode::Comment(Comment {
            data: data.to_string(),
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_mustache(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{")?;
        let expression_start = self.pos as u32;
        let expression = self.read_expression()?;
        let expression_span = Span::new(expression_start, self.pos as u32);
        self.eat("}")?;
        let expression_ast = parse_expr_into(self.allocator, &expression);
        Ok(TemplateNode::MustacheTag(MustacheTag {
            expression,
            expression_ast,
            span: Span::new(start, self.pos as u32),
            expression_span,
            _phantom: PhantomData,
        }))
    }

    fn parse_raw_mustache(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@html")?;
        self.skip_whitespace();
        let expression_start = self.pos as u32;
        let expression = self.read_expression()?;
        let expression_span = Span::new(expression_start, self.pos as u32);
        self.eat("}")?;
        Ok(TemplateNode::RawMustacheTag(RawMustacheTag {
            _phantom: PhantomData,
            expression,
            span: Span::new(start, self.pos as u32),
            expression_span,
        }))
    }

    fn parse_debug_tag(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@debug")?;
        self.skip_whitespace();
        let identifiers_start = self.pos as u32;
        let idents_str = self.eat_until("}");
        if debug_tag_has_invalid_arguments(idents_str, self.allocator) {
            self.report_error(
                "{@debug ...} arguments must be identifiers, not arbitrary expressions",
            );
        }
        let (identifiers, identifier_spans) =
            parse_debug_identifiers(Span::new(identifiers_start, self.pos as u32), idents_str);
        self.eat("}")?;
        Ok(TemplateNode::DebugTag(DebugTag {
            _phantom: PhantomData,
            identifiers,
            identifier_spans,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_const_tag(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@const")?;
        self.skip_whitespace();
        let declaration_start = self.pos as u32;
        let declaration = self.read_expression()?;
        let declaration_span = Span::new(declaration_start, self.pos as u32);
        if let Some(message) = const_tag_declaration_diagnostic(&declaration) {
            self.report_error(message);
        }
        self.eat("}")?;
        Ok(TemplateNode::ConstTag(ConstTag {
            _phantom: PhantomData,
            declaration,
            span: Span::new(start, self.pos as u32),
            declaration_span,
        }))
    }

    fn parse_render_tag(&mut self) -> Result<TemplateNode<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@render")?;
        self.skip_whitespace();
        let expression_start = self.pos as u32;
        let expression = self.read_expression()?;
        let expression_span = Span::new(expression_start, self.pos as u32);
        for diagnostic in render_tag_expression_diagnostics(&expression, self.allocator) {
            self.report_error(diagnostic);
        }
        self.eat("}")?;
        Ok(TemplateNode::RenderTag(RenderTag {
            _phantom: PhantomData,
            expression,
            span: Span::new(start, self.pos as u32),
            expression_span,
        }))
    }

    fn parse_element(&mut self, frame: FragmentFrame) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        let is_root = self.current_frame_is_root_scope(frame);
        let parent_owned = self
            .current_frame_parent_element_name()
            .map(|s| s.to_string());
        let start = self.pos as u32;
        self.eat("<")?;

        // Parse tag name (allow ! for <!doctype>)
        let name_start = self.pos;
        self.pos = scanner::read_tag_name_end(self.source, self.pos);
        let name = self.source[name_start..self.pos].to_string();
        let name_span = Span::new(name_start as u32, self.pos as u32);
        let is_head_title = name == "title" && self.in_svelte_head_context;
        self.report_tag_name_diagnostic(&name);
        self.report_svelte_meta_tag_diagnostics(&name, is_root);
        self.report_svelte_fragment_placement_diagnostic(&name, parent_owned.as_deref());
        self.report_svelte_self_placement_diagnostic(&name);

        // Parse attributes
        let (attributes, attribute_meta) = self.parse_attributes()?;
        self.report_duplicate_attributes(&attributes);
        self.report_attribute_analyzer_diagnostics(&name, &attributes);
        self.report_svelte_special_element_attribute_diagnostics(&name, &attributes);
        self.report_slot_element_attribute_diagnostics(&name, &attributes);

        // Check for self-closing or void element
        self.skip_whitespace();
        let (self_closing, start_tag_end) = if self.looking_at("/>") {
            // `>` is at self.pos + 1 (after the `/`).
            let bracket = (self.pos + 1) as u32;
            self.eat("/>")?;
            (true, bracket)
        } else if self.looking_at(">") {
            let bracket = self.pos as u32;
            self.eat(">")?;
            (false, bracket)
        } else {
            // No > found — unclosed opening tag. Treat as self-closing. The
            // bracket offset falls back to the current position so the field
            // is still valid for length arithmetic.
            self.report_error(format!("Unclosed <{name}> start tag"));
            (true, self.pos as u32)
        };

        let is_void = is_void_element(&name);
        let parse_raw_text_mustaches = name.eq_ignore_ascii_case("textarea");
        let is_raw_text = parse_raw_text_mustaches
            || name.eq_ignore_ascii_case("script")
            || name.eq_ignore_ascii_case("style");
        let context = self.enter_element_context(&name, &attributes);

        // Self-closing / void elements have no children — build immediately.
        if self_closing || is_void {
            self.exit_element_context(context);
            let children: Vec<TemplateNode<'a>> = Vec::new();
            self.report_svelte_special_element_content_diagnostics(&name, &children);
            self.report_textarea_content_diagnostics(&name, &attributes, &children);
            if is_head_title {
                self.report_head_title_diagnostics(&attributes, &children);
            }
            let mut end = self.pos as u32;
            if end as usize >= self.source.len() {
                while end > start
                    && self.source.as_bytes()[(end - 1) as usize].is_ascii_whitespace()
                {
                    end -= 1;
                }
            }
            return Ok(FragmentStep::Node(TemplateNode::Element(Element {
                name,
                name_span,
                attributes,
                attribute_meta,
                children,
                self_closing,
                span: Span::new(start, end),
                start_tag_end,
                end_tag_span: None,
                unclosed_at_eof_outer: false,
            })));
        }

        // Raw-text elements (script / style / textarea) keep their dedicated
        // scanner-based child reader; they don't produce normal template
        // children and don't participate in the open-element stack.
        if is_raw_text {
            let (children, end_tag_span) =
                match self.parse_raw_text_children(&name, parse_raw_text_mustaches) {
                    Ok(pair) => pair,
                    Err(err) => {
                        self.exit_element_context(context);
                        return Err(err);
                    }
                };
            if end_tag_span.is_none() {
                self.report_unclosed_eof(format!("`<{name}>` was left open"));
            }
            self.exit_element_context(context);
            self.report_svelte_special_element_content_diagnostics(&name, &children);
            self.report_textarea_content_diagnostics(&name, &attributes, &children);
            if is_head_title {
                self.report_head_title_diagnostics(&attributes, &children);
            }
            let mut end = self.pos as u32;
            if end as usize >= self.source.len() {
                while end > start
                    && self.source.as_bytes()[(end - 1) as usize].is_ascii_whitespace()
                {
                    end -= 1;
                }
            }
            return Ok(FragmentStep::Node(TemplateNode::Element(Element {
                name,
                name_span,
                attributes,
                attribute_meta,
                children,
                self_closing: false,
                span: Span::new(start, end),
                start_tag_end,
                end_tag_span,
                unclosed_at_eof_outer: false,
            })));
        }

        // Regular element with children: push onto the open-node stack and
        // open a fresh child fragment. The dispatch loop will continue parsing
        // children directly into that fragment, and `finalize_top_open_element`
        // will assemble the final `Element` when the close arrives.
        self.open_nodes.push(OpenNode::Element(OpenElement {
            name,
            name_span,
            attributes,
            attribute_meta,
            span_start: start,
            start_tag_end,
            is_head_title,
            context,
        }));
        self.enter_fragment();
        Ok(FragmentStep::Continue)
    }

    fn report_head_title_diagnostics(
        &mut self,
        attributes: &[Attribute],
        children: &[TemplateNode<'a>],
    ) {
        for _attribute in attributes {
            self.report_error("`<title>` cannot have attributes nor directives");
        }

        for child in children {
            if !matches!(child, TemplateNode::Text(_) | TemplateNode::MustacheTag(_)) {
                self.report_error("`<title>` can only contain text and {tags}");
            }
        }
    }

    fn report_textarea_content_diagnostics(
        &mut self,
        name: &str,
        attributes: &[Attribute],
        children: &[TemplateNode<'a>],
    ) {
        if !name.eq_ignore_ascii_case("textarea") || children.is_empty() {
            return;
        }

        if find_normal_attribute(attributes, "value").is_some() {
            self.report_error(
                "A `<textarea>` can have either a value attribute or (equivalently) child content, but not both",
            );
        }
    }

    fn report_tag_name_diagnostic(&mut self, name: &str) {
        if !is_valid_element_or_component_name(name) {
            self.report_error("Expected a valid element or component name. Components must have a valid variable name or dot notation expression");
        }
    }

    fn report_svelte_meta_tag_diagnostics(&mut self, name: &str, is_root: bool) {
        if name.starts_with("svelte:") && !is_svelte_meta_tag(name) {
            self.report_error(format!(
                "Valid `<svelte:...>` tag names are {}",
                valid_svelte_meta_tag_list()
            ));
        }

        if !is_root_only_svelte_meta_tag(name) {
            return;
        }

        if self.seen_root_meta_tags.iter().any(|seen| seen == name) {
            self.report_error(format!("A component can only have one `<{name}>` element"));
        }
        if !is_root {
            self.report_error(format!(
                "`<{name}>` tags cannot be inside elements or blocks"
            ));
        }

        self.seen_root_meta_tags.push(name.to_string());
    }

    fn report_svelte_fragment_placement_diagnostic(&mut self, name: &str, parent: Option<&str>) {
        if name != "svelte:fragment" {
            return;
        }

        if !parent.is_some_and(is_component_like_element_name) {
            self.report_error("`<svelte:fragment>` must be the direct child of a component");
        }
    }

    fn report_svelte_self_placement_diagnostic(&mut self, name: &str) {
        if name == "svelte:self" && self.svelte_self_allowed_depth == 0 {
            self.report_error(
                "`<svelte:self>` components can only exist inside `{#if}` blocks, `{#each}` blocks, `{#snippet}` blocks or slots passed to components",
            );
        }
    }

    fn report_attribute_analyzer_diagnostics(&mut self, name: &str, attributes: &[Attribute]) {
        let is_component = is_component_slot_owner_name(name);
        let is_regular_or_svelte_element = uses_regular_element_attribute_rules(name);
        let uses_bind_target_rules = uses_bind_target_rules(name);

        for attribute in attributes {
            match attribute {
                Attribute::Directive {
                    kind: DirectiveKind::Binding,
                    name: binding_name,
                    value,
                    ..
                } => {
                    self.report_bind_expression_diagnostics(binding_name, value);
                }
                Attribute::Directive {
                    kind:
                        DirectiveKind::Use
                        | DirectiveKind::Transition
                        | DirectiveKind::In
                        | DirectiveKind::Out
                        | DirectiveKind::Animate,
                    value,
                    ..
                } if is_regular_or_svelte_element => {
                    self.report_illegal_await_expression_diagnostic(value);
                }
                Attribute::NormalAttribute { name, value, .. } if name == "@attach" => {
                    self.report_illegal_await_expression_diagnostic(value);
                }
                _ => {}
            }

            match attribute {
                Attribute::NormalAttribute { name, .. }
                    if is_regular_or_svelte_element
                        && name != "@attach"
                        && attribute_name_is_invalid(name) =>
                {
                    self.report_error(format!("'{name}' is not a valid attribute name"));
                }
                Attribute::NormalAttribute { name, value, .. }
                    if is_regular_or_svelte_element
                        && name.starts_with("on")
                        && name.len() > 2
                        && !is_expression_attribute_value(value) =>
                {
                    self.report_error(
                        "Event attribute must be a JavaScript expression, not a string",
                    );
                }
                Attribute::Directive {
                    kind: DirectiveKind::EventHandler,
                    modifiers,
                    ..
                } if is_component => {
                    if modifiers.len() > 1 || modifiers.iter().any(|modifier| modifier != "once") {
                        self.report_error(
                            "Event modifiers other than 'once' can only be used on DOM elements",
                        );
                    }
                }
                Attribute::Directive { kind, .. } if is_component => {
                    if component_directive_is_invalid(kind) {
                        self.report_error(component_invalid_directive_message());
                    }
                }
                Attribute::Directive {
                    kind: DirectiveKind::EventHandler,
                    modifiers,
                    ..
                } if is_regular_or_svelte_element => {
                    self.report_event_modifier_diagnostics(modifiers);
                }
                Attribute::Directive {
                    kind: DirectiveKind::Binding,
                    name: binding_name,
                    value: _,
                    ..
                } if uses_bind_target_rules => {
                    self.report_bind_directive_diagnostics(name, attributes, binding_name);
                }
                Attribute::Directive {
                    kind: DirectiveKind::StyleDirective,
                    modifiers,
                    ..
                } if is_regular_or_svelte_element => {
                    if modifiers.iter().any(|modifier| modifier != "important") {
                        self.report_error(
                            "`style:` directive can only use the `important` modifier",
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn report_bind_expression_diagnostics(&mut self, binding_name: &str, value: &AttributeValue) {
        let Some(raw_expression) = single_expression_attribute_text(value) else {
            return;
        };

        for diagnostic in bind_expression_diagnostics(raw_expression, binding_name, self.allocator)
        {
            self.report_error(diagnostic);
        }
    }

    fn report_illegal_await_expression_diagnostic(&mut self, value: &AttributeValue) {
        let Some(raw_expression) = single_expression_attribute_text(value) else {
            return;
        };

        if expression_text_has_await_outside_functions(raw_expression, self.allocator) {
            self.report_error(illegal_await_expression_message());
        }
    }

    fn report_bind_directive_diagnostics(
        &mut self,
        element_name: &str,
        attributes: &[Attribute],
        binding_name: &str,
    ) {
        if !is_known_binding_property(binding_name) {
            let mut message = format!("`bind:{binding_name}` is not a valid binding");
            if let Some(suggestion) = binding_name_suggestion(binding_name, element_name) {
                message.push_str(&format!(". Did you mean '{suggestion}'?"));
            }
            self.report_error(message);
            return;
        }

        if let Some(valid_elements) = binding_valid_elements(binding_name) {
            if !valid_elements.contains(&element_name) {
                self.report_error(format!(
                    "`bind:{binding_name}` can only be used with {}",
                    html_tag_list(valid_elements)
                ));
                return;
            }
        }

        if let Some(invalid_elements) = binding_invalid_elements(binding_name) {
            if invalid_elements.contains(&element_name) {
                self.report_error(format!(
                    "`bind:{binding_name}` is not a valid binding. Possible bindings for <{element_name}> are {}",
                    possible_bindings_for_element(element_name).join(", ")
                ));
                return;
            }
        }

        if binding_name == "offsetWidth" && is_svg_element_name(element_name) {
            self.report_error(
                "`bind:offsetWidth` can only be used with non-`<svg>` elements. Use `bind:clientWidth` for `<svg>` instead",
            );
            return;
        }

        if element_name == "input" && binding_name != "this" {
            self.report_input_binding_diagnostics(attributes, binding_name);
        }

        if element_name == "select" && binding_name != "this" {
            if let Some(Attribute::NormalAttribute { value, .. }) =
                find_normal_attribute(attributes, "multiple")
            {
                if !matches!(value, AttributeValue::True | AttributeValue::Static(_)) {
                    self.report_error(
                        "'multiple' attribute must be static if select uses two-way binding",
                    );
                }
            }
        }

        if is_contenteditable_binding(binding_name) {
            match find_normal_attribute(attributes, "contenteditable") {
                None => self.report_error(
                    "'contenteditable' attribute is required for textContent, innerHTML and innerText two-way bindings",
                ),
                Some(Attribute::NormalAttribute { value, .. })
                    if !matches!(value, AttributeValue::True | AttributeValue::Static(_)) =>
                {
                    self.report_error(
                        "'contenteditable' attribute cannot be dynamic if element uses two-way binding",
                    );
                }
                _ => {}
            }
        }
    }

    fn report_input_binding_diagnostics(&mut self, attributes: &[Attribute], binding_name: &str) {
        let type_value =
            find_normal_attribute(attributes, "type").and_then(|attribute| match attribute {
                Attribute::NormalAttribute { value, .. } => Some(value),
                _ => None,
            });

        match type_value {
            Some(AttributeValue::Static(input_type)) => {
                if binding_name == "checked" && input_type != "checkbox" {
                    let mut target = "`<input type=\"checkbox\">`".to_string();
                    if input_type == "radio" {
                        target.push_str(" - for `<input type=\"radio\">`, use `bind:group`");
                    }
                    self.report_error(format!("`bind:checked` can only be used with {target}"));
                }
                if binding_name == "files" && input_type != "file" {
                    self.report_error("`bind:files` can only be used with `<input type=\"file\">`");
                }
            }
            Some(AttributeValue::True) => {
                self.report_error(
                    "'type' attribute must be a static text value if input uses two-way binding",
                );
            }
            Some(AttributeValue::Expression(_) | AttributeValue::Concat(_))
                if binding_name != "value" =>
            {
                self.report_error(
                    "'type' attribute must be a static text value if input uses two-way binding",
                );
            }
            _ => {
                if binding_name == "checked" {
                    self.report_error(
                        "`bind:checked` can only be used with `<input type=\"checkbox\">`",
                    );
                }
                if binding_name == "files" {
                    self.report_error("`bind:files` can only be used with `<input type=\"file\">`");
                }
            }
        }
    }

    fn report_event_modifier_diagnostics(&mut self, modifiers: &[String]) {
        let mut has_passive_modifier = false;
        let mut conflicting_passive_modifier = "";

        for modifier in modifiers {
            if !is_valid_event_modifier(modifier) {
                self.report_error(event_handler_invalid_modifier_message());
            }
            if modifier == "passive" {
                has_passive_modifier = true;
            } else if modifier == "nonpassive" || modifier == "preventDefault" {
                conflicting_passive_modifier = modifier;
            }
            if has_passive_modifier && !conflicting_passive_modifier.is_empty() {
                self.report_error(format!(
                    "The 'passive' and '{conflicting_passive_modifier}' modifiers cannot be used together"
                ));
            }
        }
    }

    fn report_svelte_special_element_attribute_diagnostics(
        &mut self,
        name: &str,
        attributes: &[Attribute],
    ) {
        match name {
            "svelte:options" => {
                self.report_svelte_options_attribute_diagnostics(attributes);
            }
            "svelte:head" => {
                for _attribute in attributes {
                    self.report_error("`<svelte:head>` cannot have attributes nor directives");
                }
            }
            "svelte:body" | "svelte:window" | "svelte:document" => {
                for attribute in attributes {
                    if matches!(
                        attribute,
                        Attribute::Directive {
                            kind: DirectiveKind::Let,
                            ..
                        }
                    ) {
                        self.report_error("`let:` directive at invalid position");
                    } else if is_invalid_svelte_event_target_attribute(attribute) {
                        self.report_error(format!(
                            "`<{name}>` does not support non-event attributes or spread attributes"
                        ));
                    }
                }
            }
            "svelte:component" => match find_this_attribute_value(attributes) {
                Some(value) if !is_expression_attribute_value(value) => {
                    self.report_error("Invalid component definition — must be an `{expression}`");
                }
                Some(_) => {}
                None => {
                    self.report_error("`<svelte:component>` must have a 'this' attribute");
                }
            },
            "svelte:element" => match find_this_attribute_value(attributes) {
                Some(AttributeValue::True) | None => {
                    self.report_error(
                        "`<svelte:element>` must have a 'this' attribute with a value",
                    );
                }
                Some(_) => {}
            },
            "svelte:fragment" => {
                for attribute in attributes {
                    if !is_valid_svelte_fragment_attribute(attribute) {
                        self.report_error(
                            "`<svelte:fragment>` can only have a slot attribute and (optionally) a let: directive",
                        );
                    }
                }
            }
            "svelte:boundary" => {
                for attribute in attributes {
                    match attribute {
                        Attribute::NormalAttribute { name, value, .. } => {
                            if !is_valid_svelte_boundary_attribute_name(name) {
                                self.report_error(
                                    "Valid attributes on `<svelte:boundary>` are `onerror` and `failed`",
                                );
                            }
                            if !is_expression_attribute_value(value) {
                                self.report_error(
                                    "Attribute value must be a non-string expression",
                                );
                            }
                        }
                        Attribute::Directive { .. } | Attribute::Spread { .. } => {
                            self.report_error(
                                "Valid attributes on `<svelte:boundary>` are `onerror` and `failed`",
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn report_svelte_options_attribute_diagnostics(&mut self, attributes: &[Attribute]) {
        for attribute in attributes {
            let Attribute::NormalAttribute { name, value, .. } = attribute else {
                self.report_error("`<svelte:options>` can only receive static attributes");
                continue;
            };

            match name.as_str() {
                "runes" | "immutable" | "preserveWhitespace" | "accessors" => {
                    if !matches!(static_option_value(value), Some(StaticOptionValue::Bool)) {
                        self.report_error("Value must be true or false, if specified");
                    }
                }
                "namespace" => match static_option_value(value) {
                    Some(StaticOptionValue::String(value))
                        if matches!(
                            value.as_str(),
                            "html"
                                | "mathml"
                                | "svg"
                                | "http://www.w3.org/2000/svg"
                                | "http://www.w3.org/1998/Math/MathML"
                        ) => {}
                    _ => {
                        self.report_error(
                            "Value must be \"html\", \"mathml\" or \"svg\", if specified",
                        );
                    }
                },
                "css" => match static_option_value(value) {
                    Some(StaticOptionValue::String(value)) if value == "injected" => {}
                    _ => {
                        self.report_error("Value must be \"injected\", if specified");
                    }
                },
                "tag" => {
                    self.report_error(
                        "\"tag\" option is deprecated — use \"customElement\" instead",
                    );
                }
                "customElement" => {
                    self.report_custom_element_option_diagnostics(value);
                }
                _ => {
                    self.report_error(format!("`<svelte:options>` unknown attribute '{name}'"));
                }
            }
        }
    }

    fn report_slot_element_attribute_diagnostics(&mut self, name: &str, attributes: &[Attribute]) {
        if name != "slot" || self.shadowroot_template_depth > 0 {
            return;
        }

        for attribute in attributes {
            match attribute {
                Attribute::NormalAttribute { name, value, .. } if name == "name" => {
                    let Some(slot_name) = static_text_attribute_value(value) else {
                        self.report_error("slot attribute must be a static value");
                        continue;
                    };
                    if slot_name == "default" {
                        self.report_error(
                            "`default` is a reserved word — it cannot be used as a slot name",
                        );
                    }
                }
                Attribute::NormalAttribute { .. } | Attribute::Spread { .. } => {}
                Attribute::Directive { kind, .. } if matches!(kind, DirectiveKind::Let) => {}
                Attribute::Directive { .. } => {
                    self.report_error(
                        "`<slot>` can only receive attributes and (optionally) let directives",
                    );
                }
            }
        }
    }

    fn report_slot_attribute_tree_diagnostics(
        &mut self,
        nodes: &[TemplateNode<'a>],
        ancestors: &mut Vec<SlotAncestor>,
    ) {
        for node in nodes {
            match node {
                TemplateNode::Element(element) => {
                    self.report_slot_attribute_diagnostics(element, ancestors);
                    self.report_component_slot_children_diagnostics(element);
                    self.report_component_snippet_children_diagnostics(element);

                    ancestors.push(SlotAncestor {
                        kind: slot_ancestor_kind(element),
                    });
                    self.report_slot_attribute_tree_diagnostics(&element.children, ancestors);
                    ancestors.pop();
                }
                TemplateNode::IfBlock(block) => {
                    ancestors.push(SlotAncestor::block());
                    self.report_slot_attribute_tree_diagnostics(&block.consequent.nodes, ancestors);
                    ancestors.pop();

                    if let Some(alternate) = &block.alternate {
                        ancestors.push(SlotAncestor::block());
                        self.report_slot_attribute_node_diagnostics(alternate, ancestors);
                        ancestors.pop();
                    }
                }
                TemplateNode::EachBlock(block) => {
                    ancestors.push(SlotAncestor::block());
                    self.report_slot_attribute_tree_diagnostics(&block.body.nodes, ancestors);
                    ancestors.pop();

                    if let Some(fallback) = &block.fallback {
                        ancestors.push(SlotAncestor::block());
                        self.report_slot_attribute_tree_diagnostics(&fallback.nodes, ancestors);
                        ancestors.pop();
                    }
                }
                TemplateNode::AwaitBlock(block) => {
                    for fragment in await_block_fragments(block) {
                        ancestors.push(SlotAncestor::block());
                        self.report_slot_attribute_tree_diagnostics(&fragment.nodes, ancestors);
                        ancestors.pop();
                    }
                }
                TemplateNode::KeyBlock(block) => {
                    ancestors.push(SlotAncestor::block());
                    self.report_slot_attribute_tree_diagnostics(&block.body.nodes, ancestors);
                    ancestors.pop();
                }
                TemplateNode::SnippetBlock(block) => {
                    ancestors.push(SlotAncestor::snippet_block());
                    self.report_slot_attribute_tree_diagnostics(&block.body.nodes, ancestors);
                    ancestors.pop();
                }
                TemplateNode::Text(_)
                | TemplateNode::MustacheTag(_)
                | TemplateNode::RawMustacheTag(_)
                | TemplateNode::DebugTag(_)
                | TemplateNode::ConstTag(_)
                | TemplateNode::RenderTag(_)
                | TemplateNode::Comment(_) => {}
            }
        }
    }

    fn report_slot_attribute_node_diagnostics(
        &mut self,
        node: &TemplateNode<'a>,
        ancestors: &mut Vec<SlotAncestor>,
    ) {
        self.report_slot_attribute_tree_diagnostics(std::slice::from_ref(node), ancestors);
    }

    fn report_const_tag_placement_diagnostics(
        &mut self,
        nodes: &[TemplateNode<'a>],
        const_allowed: bool,
    ) {
        for node in nodes {
            self.report_const_tag_placement_node_diagnostics(node, const_allowed);
        }
    }

    fn report_const_tag_placement_node_diagnostics(
        &mut self,
        node: &TemplateNode<'a>,
        const_allowed: bool,
    ) {
        match node {
            TemplateNode::ConstTag(_) if !const_allowed => {
                self.report_error(const_tag_invalid_placement_message());
            }
            TemplateNode::Element(element) => {
                self.report_const_tag_placement_diagnostics(
                    &element.children,
                    const_tag_allowed_in_element(element),
                );
            }
            TemplateNode::IfBlock(block) => {
                self.report_const_tag_placement_diagnostics(&block.consequent.nodes, true);
                if let Some(alternate) = &block.alternate {
                    self.report_const_tag_placement_node_diagnostics(alternate, true);
                }
            }
            TemplateNode::EachBlock(block) => {
                self.report_const_tag_placement_diagnostics(&block.body.nodes, true);
                if let Some(fallback) = &block.fallback {
                    self.report_const_tag_placement_diagnostics(&fallback.nodes, true);
                }
            }
            TemplateNode::AwaitBlock(block) => {
                for fragment in await_block_fragments(block) {
                    self.report_const_tag_placement_diagnostics(&fragment.nodes, true);
                }
            }
            TemplateNode::KeyBlock(block) => {
                self.report_const_tag_placement_diagnostics(&block.body.nodes, true);
            }
            TemplateNode::SnippetBlock(block) => {
                self.report_const_tag_placement_diagnostics(&block.body.nodes, true);
            }
            TemplateNode::Text(_)
            | TemplateNode::MustacheTag(_)
            | TemplateNode::RawMustacheTag(_)
            | TemplateNode::DebugTag(_)
            | TemplateNode::ConstTag(_)
            | TemplateNode::RenderTag(_)
            | TemplateNode::Comment(_) => {}
        }
    }

    fn report_text_placement_diagnostics(
        &mut self,
        nodes: &[TemplateNode<'a>],
        parent_element: Option<&str>,
    ) {
        for node in nodes {
            self.report_text_placement_node_diagnostics(node, parent_element);
        }
    }

    fn report_text_placement_node_diagnostics(
        &mut self,
        node: &TemplateNode<'a>,
        parent_element: Option<&str>,
    ) {
        match node {
            TemplateNode::Text(text) if !text.data.trim().is_empty() => {
                self.report_html_text_placement(parent_element);
            }
            TemplateNode::MustacheTag(_) => {
                self.report_html_text_placement(parent_element);
            }
            TemplateNode::Element(element) => {
                let next_parent = text_placement_parent_for_element(element, parent_element);
                self.report_text_placement_diagnostics(&element.children, next_parent);
            }
            TemplateNode::IfBlock(block) => {
                self.report_text_placement_diagnostics(&block.consequent.nodes, parent_element);
                if let Some(alternate) = &block.alternate {
                    self.report_text_placement_node_diagnostics(alternate, parent_element);
                }
            }
            TemplateNode::EachBlock(block) => {
                self.report_text_placement_diagnostics(&block.body.nodes, parent_element);
                if let Some(fallback) = &block.fallback {
                    self.report_text_placement_diagnostics(&fallback.nodes, parent_element);
                }
            }
            TemplateNode::AwaitBlock(block) => {
                for fragment in await_block_fragments(block) {
                    self.report_text_placement_diagnostics(&fragment.nodes, parent_element);
                }
            }
            TemplateNode::KeyBlock(block) => {
                self.report_text_placement_diagnostics(&block.body.nodes, parent_element);
            }
            TemplateNode::SnippetBlock(block) => {
                self.report_text_placement_diagnostics(&block.body.nodes, None);
            }
            TemplateNode::RawMustacheTag(_)
            | TemplateNode::DebugTag(_)
            | TemplateNode::ConstTag(_)
            | TemplateNode::RenderTag(_)
            | TemplateNode::Comment(_)
            | TemplateNode::Text(_) => {}
        }
    }

    fn report_html_text_placement(&mut self, parent_element: Option<&str>) {
        let Some(parent_element) = parent_element else {
            return;
        };
        if let Some(message) = html_text_invalid_placement_message(parent_element) {
            self.report_error(node_invalid_placement_message(message));
        }
    }

    fn report_element_placement_diagnostics(
        &mut self,
        nodes: &[TemplateNode<'a>],
        ancestors: &[HtmlPlacementAncestor<'_>],
    ) {
        for node in nodes {
            self.report_element_placement_node_diagnostics(node, ancestors);
        }
    }

    fn report_element_placement_node_diagnostics(
        &mut self,
        node: &TemplateNode<'a>,
        ancestors: &[HtmlPlacementAncestor<'_>],
    ) {
        match node {
            TemplateNode::Element(element) => {
                if element_participates_in_html_placement(&element.name) {
                    self.report_html_element_placement(&element.name, ancestors);
                }

                if element.name == "slot" || element.name == "svelte:boundary" {
                    self.report_element_placement_diagnostics(&element.children, ancestors);
                } else if element.name.starts_with("svelte:")
                    || is_regular_component_element_name(&element.name)
                {
                    self.report_element_placement_diagnostics(&element.children, &[]);
                } else {
                    let mut next_ancestors = Vec::with_capacity(ancestors.len() + 1);
                    next_ancestors.push(HtmlPlacementAncestor {
                        name: element.name.as_str(),
                        blocked_by_block: false,
                    });
                    next_ancestors.extend_from_slice(ancestors);
                    self.report_element_placement_diagnostics(&element.children, &next_ancestors);
                }
            }
            TemplateNode::IfBlock(block) => {
                let blocked = html_placement_ancestors_blocked(ancestors);
                self.report_element_placement_diagnostics(&block.consequent.nodes, &blocked);
                if let Some(alternate) = &block.alternate {
                    self.report_element_placement_node_diagnostics(alternate, &blocked);
                }
            }
            TemplateNode::EachBlock(block) => {
                let blocked = html_placement_ancestors_blocked(ancestors);
                self.report_element_placement_diagnostics(&block.body.nodes, &blocked);
                if let Some(fallback) = &block.fallback {
                    self.report_element_placement_diagnostics(&fallback.nodes, &blocked);
                }
            }
            TemplateNode::AwaitBlock(block) => {
                let blocked = html_placement_ancestors_blocked(ancestors);
                for fragment in await_block_fragments(block) {
                    self.report_element_placement_diagnostics(&fragment.nodes, &blocked);
                }
            }
            TemplateNode::KeyBlock(block) => {
                let blocked = html_placement_ancestors_blocked(ancestors);
                self.report_element_placement_diagnostics(&block.body.nodes, &blocked);
            }
            TemplateNode::SnippetBlock(block) => {
                self.report_element_placement_diagnostics(&block.body.nodes, &[]);
            }
            TemplateNode::Text(_)
            | TemplateNode::MustacheTag(_)
            | TemplateNode::RawMustacheTag(_)
            | TemplateNode::DebugTag(_)
            | TemplateNode::ConstTag(_)
            | TemplateNode::RenderTag(_)
            | TemplateNode::Comment(_) => {}
        }
    }

    fn report_html_element_placement(
        &mut self,
        child: &str,
        ancestors: &[HtmlPlacementAncestor<'_>],
    ) {
        if let Some(parent) = ancestors.first() {
            if !parent.blocked_by_block {
                if let Some(message) = html_tag_invalid_parent_message(child, parent.name) {
                    self.report_error(node_invalid_placement_message(message));
                }
            }
        }

        for (index, ancestor) in ancestors.iter().enumerate().skip(1) {
            if ancestor.blocked_by_block {
                continue;
            }
            if let Some(message) = html_tag_invalid_ancestor_message(child, ancestors, index) {
                self.report_error(node_invalid_placement_message(message));
            }
        }
    }

    fn report_motion_directive_diagnostics(
        &mut self,
        nodes: &[TemplateNode<'a>],
        each_context: Option<EachMotionContext>,
    ) {
        for node in nodes {
            self.report_motion_directive_node_diagnostics(node, each_context);
        }
    }

    fn report_motion_directive_node_diagnostics(
        &mut self,
        node: &TemplateNode<'a>,
        each_context: Option<EachMotionContext>,
    ) {
        match node {
            TemplateNode::Element(element) => {
                if uses_regular_element_attribute_rules(&element.name) {
                    self.report_element_motion_directives(&element.attributes, each_context);
                }
                self.report_motion_directive_diagnostics(&element.children, None);
            }
            TemplateNode::IfBlock(block) => {
                self.report_motion_directive_diagnostics(&block.consequent.nodes, None);
                if let Some(alternate) = &block.alternate {
                    self.report_motion_directive_node_diagnostics(alternate, None);
                }
            }
            TemplateNode::EachBlock(block) => {
                let context = EachMotionContext {
                    keyed: block.key.as_ref().is_some_and(|key| !key.trim().is_empty()),
                    significant_body_children: motion_significant_child_count(&block.body.nodes),
                };
                self.report_motion_directive_diagnostics(&block.body.nodes, Some(context));
                if let Some(fallback) = &block.fallback {
                    self.report_motion_directive_diagnostics(&fallback.nodes, Some(context));
                }
            }
            TemplateNode::AwaitBlock(block) => {
                for fragment in await_block_fragments(block) {
                    self.report_motion_directive_diagnostics(&fragment.nodes, None);
                }
            }
            TemplateNode::KeyBlock(block) => {
                self.report_motion_directive_diagnostics(&block.body.nodes, None);
            }
            TemplateNode::SnippetBlock(block) => {
                self.report_motion_directive_diagnostics(&block.body.nodes, None);
            }
            TemplateNode::Text(_)
            | TemplateNode::MustacheTag(_)
            | TemplateNode::RawMustacheTag(_)
            | TemplateNode::DebugTag(_)
            | TemplateNode::ConstTag(_)
            | TemplateNode::RenderTag(_)
            | TemplateNode::Comment(_) => {}
        }
    }

    fn report_element_motion_directives(
        &mut self,
        attributes: &[Attribute],
        each_context: Option<EachMotionContext>,
    ) {
        self.report_animation_directive_diagnostics(attributes, each_context);
        self.report_transition_directive_diagnostics(attributes);
    }

    fn report_animation_directive_diagnostics(
        &mut self,
        attributes: &[Attribute],
        each_context: Option<EachMotionContext>,
    ) {
        let mut has_animate_directive = false;

        for attribute in attributes {
            let Attribute::Directive {
                kind: DirectiveKind::Animate,
                ..
            } = attribute
            else {
                continue;
            };

            let Some(each_context) = each_context else {
                self.report_error(animation_invalid_placement_message());
                continue;
            };
            if !each_context.keyed {
                self.report_error(animation_missing_key_message());
                continue;
            }
            if each_context.significant_body_children > 1 {
                self.report_error(animation_invalid_placement_message());
                continue;
            }

            if has_animate_directive {
                self.report_error("An element can only have one 'animate' directive");
            } else {
                has_animate_directive = true;
            }
        }
    }

    fn report_transition_directive_diagnostics(&mut self, attributes: &[Attribute]) {
        let mut in_transition = None;
        let mut out_transition = None;

        for attribute in attributes {
            let Some(current) = transition_directive_type(attribute) else {
                continue;
            };
            let intro = matches!(current, "transition" | "in");
            let outro = matches!(current, "transition" | "out");
            let existing = if intro {
                in_transition
            } else if outro {
                out_transition
            } else {
                None
            };

            if let Some(existing) = existing {
                if existing == current {
                    self.report_error(format!(
                        "Cannot use multiple `{current}:` directives on a single element"
                    ));
                } else {
                    self.report_error(format!(
                        "Cannot use `{existing}:` alongside existing `{current}:` directive"
                    ));
                }
            }

            if intro {
                in_transition = Some(current);
            }
            if outro {
                out_transition = Some(current);
            }
        }
    }

    fn report_runes_attribute_value_diagnostics(&mut self, nodes: &[TemplateNode<'a>]) {
        walk_template_nodes(nodes, &mut |node| {
            if let TemplateNode::Element(element) = node {
                self.report_element_runes_attribute_value_diagnostics(element);
            }
        });
    }

    fn report_element_runes_attribute_value_diagnostics(&mut self, element: &Element<'a>) {
        if !uses_runes_attribute_value_rules(&element.name, &element.attributes) {
            return;
        }

        for (attribute, meta) in element.attributes.iter().zip(&element.attribute_meta) {
            let Attribute::NormalAttribute { name, value, .. } = attribute else {
                continue;
            };
            if name == "@attach" {
                continue;
            }

            if attribute_value_is_unquoted_sequence(self.source, value, meta) {
                self.report_error(attribute_unquoted_sequence_message());
                continue;
            }

            if attribute_value_is_unparenthesized_sequence_expression(value, self.allocator) {
                self.report_error(attribute_invalid_sequence_expression_message());
            }
        }
    }

    fn report_slot_snippet_conflict_diagnostics(&mut self, nodes: &[TemplateNode<'a>]) {
        let mut usage = SlotSnippetUsage::default();
        collect_slot_snippet_usage(nodes, &mut usage, false, self.allocator);

        if usage.uses_render_tags
            && (usage.uses_slots_identifier
                || (!root_custom_element_option_enabled(nodes) && usage.uses_slot_element))
        {
            self.report_error(slot_snippet_conflict_message());
        }
    }

    fn report_mixed_event_handler_syntax_diagnostics(&mut self, nodes: &[TemplateNode<'a>]) {
        let mut usage = EventSyntaxUsage::default();
        collect_event_syntax_usage(nodes, &mut usage);

        if usage.uses_event_attribute {
            if let Some(name) = usage.first_event_directive_name {
                self.report_error(mixed_event_handler_syntax_message(&name));
            }
        }
    }

    fn report_invalid_await_usage_diagnostics(&mut self, nodes: &[TemplateNode<'a>]) {
        let allocator = self.allocator;
        walk_template_nodes(nodes, &mut |node| match node {
            TemplateNode::Element(element) => {
                for attribute in &element.attributes {
                    if attribute_value_has_await_outside_functions(
                        attribute_value(attribute),
                        allocator,
                    ) {
                        self.report_error(experimental_async_message());
                    }
                }
            }
            TemplateNode::MustacheTag(tag) => {
                if expression_text_has_await_outside_functions(&tag.expression, allocator) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::RawMustacheTag(tag) => {
                if expression_text_has_await_outside_functions(&tag.expression, allocator) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::ConstTag(tag) => {
                if const_declaration_has_await_outside_functions(&tag.declaration) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::RenderTag(tag) => {
                if expression_text_has_await_outside_functions(&tag.expression, allocator) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::IfBlock(block) => {
                if expression_text_has_await_outside_functions(&block.test, allocator) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::EachBlock(block) => {
                if expression_text_has_await_outside_functions(&block.expression, allocator)
                    || block.key.as_ref().is_some_and(|key| {
                        expression_text_has_await_outside_functions(key, allocator)
                    })
                {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::AwaitBlock(block) => {
                if expression_text_has_await_outside_functions(&block.expression, allocator) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::KeyBlock(block) => {
                if expression_text_has_await_outside_functions(&block.expression, allocator) {
                    self.report_error(experimental_async_message());
                }
            }
            TemplateNode::SnippetBlock(_)
            | TemplateNode::DebugTag(_)
            | TemplateNode::Text(_)
            | TemplateNode::Comment(_) => {}
        });
    }

    fn report_invalid_arguments_usage_diagnostics(&mut self, nodes: &[TemplateNode<'a>]) {
        let allocator = self.allocator;
        walk_template_nodes(nodes, &mut |node| match node {
            TemplateNode::Element(element) => {
                for attribute in &element.attributes {
                    if attribute_value_has_invalid_arguments_usage(
                        attribute_value(attribute),
                        allocator,
                    ) {
                        self.report_error(invalid_arguments_usage_message());
                    }
                }
            }
            TemplateNode::MustacheTag(tag) => {
                if expression_text_has_invalid_arguments_usage(&tag.expression, allocator) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::RawMustacheTag(tag) => {
                if expression_text_has_invalid_arguments_usage(&tag.expression, allocator) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::ConstTag(tag) => {
                if const_declaration_has_invalid_arguments_usage(&tag.declaration) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::DebugTag(tag) => {
                if tag
                    .identifiers
                    .iter()
                    .any(|identifier| identifier == "arguments")
                {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::RenderTag(tag) => {
                if expression_text_has_invalid_arguments_usage(&tag.expression, allocator) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::IfBlock(block) => {
                if expression_text_has_invalid_arguments_usage(&block.test, allocator) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::EachBlock(block) => {
                if expression_text_has_invalid_arguments_usage(&block.expression, allocator)
                    || block.key.as_ref().is_some_and(|key| {
                        expression_text_has_invalid_arguments_usage(key, allocator)
                    })
                {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::AwaitBlock(block) => {
                if expression_text_has_invalid_arguments_usage(&block.expression, allocator) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::KeyBlock(block) => {
                if expression_text_has_invalid_arguments_usage(&block.expression, allocator) {
                    self.report_error(invalid_arguments_usage_message());
                }
            }
            TemplateNode::SnippetBlock(_) | TemplateNode::Text(_) | TemplateNode::Comment(_) => {}
        });
    }

    fn report_slot_attribute_diagnostics(
        &mut self,
        element: &Element<'a>,
        ancestors: &[SlotAncestor],
    ) {
        if element.name == "slot" {
            return;
        }

        let Some(value) = find_slot_attribute_value(&element.attributes) else {
            return;
        };

        if ancestors
            .last()
            .is_some_and(|ancestor| ancestor.kind == SlotAncestorKind::SnippetBlock)
        {
            if static_text_attribute_value(value).is_none() {
                self.report_error("slot attribute must be a static value");
            }
            return;
        }

        let current_is_component = is_component_slot_owner_name(&element.name);
        let owner = ancestors
            .iter()
            .enumerate()
            .rev()
            .find(|(_, ancestor)| ancestor.kind.is_slot_owner());

        let Some((owner_index, owner)) = owner else {
            if !current_is_component {
                self.report_error(slot_attribute_invalid_placement_message());
            }
            return;
        };

        match owner.kind {
            SlotAncestorKind::Component => {
                let direct_child_of_owner = owner_index + 1 == ancestors.len();
                if direct_child_of_owner {
                    if static_text_attribute_value(value).is_none() {
                        self.report_error("slot attribute must be a static value");
                    }
                } else if !current_is_component {
                    self.report_error(slot_attribute_invalid_placement_message());
                }
            }
            SlotAncestorKind::SvelteElement | SlotAncestorKind::CustomElement => {}
            SlotAncestorKind::RegularElement
            | SlotAncestorKind::Block
            | SlotAncestorKind::SnippetBlock => {}
        }
    }

    fn report_component_slot_children_diagnostics(&mut self, owner: &Element<'a>) {
        if !is_component_slot_owner_name(&owner.name) {
            return;
        }

        let mut seen_slots = Vec::new();
        for child in owner.children.iter().filter_map(template_node_element) {
            if child.name == "slot" {
                continue;
            }

            let Some(value) = find_slot_attribute_value(&child.attributes) else {
                continue;
            };
            let Some(slot_name) = static_text_attribute_value(value) else {
                continue;
            };

            if seen_slots.iter().any(|seen| *seen == slot_name) {
                self.report_error(format!(
                    "Duplicate slot name '{slot_name}' in <{}>",
                    owner.name
                ));
            } else {
                seen_slots.push(slot_name);
            }

            if slot_name == "default" {
                self.report_default_slot_conflicts(&owner.children);
            }
        }
    }

    fn report_default_slot_conflicts(&mut self, children: &[TemplateNode<'a>]) {
        for child in children {
            if default_slot_child_is_allowed(child) {
                continue;
            }

            self.report_error("Found default slot content alongside an explicit slot=\"default\"");
        }
    }

    fn report_component_snippet_children_diagnostics(&mut self, owner: &Element<'a>) {
        if !is_component_slot_owner_name(&owner.name) {
            return;
        }

        for child in &owner.children {
            let TemplateNode::SnippetBlock(snippet) = child else {
                continue;
            };

            if component_has_attribute_or_binding(owner, &snippet.name) {
                self.report_error(format!(
                    "This snippet is shadowing the prop `{}` with the same name",
                    snippet.name
                ));
            }

            if snippet.name == "children" && component_has_implicit_children(&owner.children) {
                self.report_error(snippet_conflict_message());
            }
        }
    }

    fn report_custom_element_option_diagnostics(&mut self, value: &AttributeValue) {
        if let AttributeValue::Static(tag) = value {
            if let Some(message) = custom_element_tag_error(tag) {
                self.report_error(message);
            }
            return;
        }

        let Some(expression) = single_expression_attribute_text(value) else {
            self.report_error(custom_element_invalid_message());
            return;
        };

        let parsed =
            crate::parser::expression::parse_template_expression(expression.trim(), self.allocator);
        let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed)
        else {
            self.report_error(custom_element_invalid_message());
            return;
        };
        if !parsed.errors.is_empty() {
            self.report_error(custom_element_invalid_message());
            return;
        }

        let expression = strip_parentheses(expression);
        match expression {
            oxc::ast::ast::Expression::NullLiteral(_) => {}
            oxc::ast::ast::Expression::ObjectExpression(object) => {
                if let Some(message) = custom_element_object_error(object) {
                    self.report_error(message);
                }
            }
            _ => {
                self.report_error(custom_element_invalid_message());
            }
        }
    }

    fn report_svelte_special_element_content_diagnostics(
        &mut self,
        name: &str,
        children: &[TemplateNode<'a>],
    ) {
        if children.is_empty() || !disallows_svelte_meta_children(name) {
            return;
        }

        self.report_error(format!("<{name}> cannot have children"));
    }

    fn report_post_parse_diagnostics(&mut self, nodes: &[TemplateNode<'a>]) {
        self.report_const_tag_placement_diagnostics(nodes, false);
        self.report_text_placement_diagnostics(nodes, None);
        self.report_element_placement_diagnostics(nodes, &[]);
        self.report_motion_directive_diagnostics(nodes, None);
        if root_runes_option_enabled(nodes) {
            self.report_runes_attribute_value_diagnostics(nodes);
        }
        self.report_slot_snippet_conflict_diagnostics(nodes);
        self.report_mixed_event_handler_syntax_diagnostics(nodes);
        self.report_invalid_await_usage_diagnostics(nodes);
        self.report_invalid_arguments_usage_diagnostics(nodes);

        let mut ancestors = Vec::new();
        self.report_slot_attribute_tree_diagnostics(nodes, &mut ancestors);
    }

    fn parse_attributes(&mut self) -> Result<(Vec<Attribute>, Vec<AttributeMeta>), OxcDiagnostic> {
        let mut attributes = Vec::new();
        let mut attribute_meta = Vec::new();

        loop {
            self.skip_whitespace();

            // Skip JS-style comments between attributes
            loop {
                if self.looking_at("//") {
                    // Line comment: skip to end of line
                    while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != b'\n'
                    {
                        self.pos += 1;
                    }
                    self.skip_whitespace();
                } else if self.looking_at("/*") {
                    // Block comment: skip to */
                    self.pos += 2;
                    while self.pos + 1 < self.source.len() {
                        if self.source.as_bytes()[self.pos] == b'*'
                            && self.source.as_bytes()[self.pos + 1] == b'/'
                        {
                            self.pos += 2;
                            break;
                        }
                        self.pos += 1;
                    }
                    self.skip_whitespace();
                } else {
                    break;
                }
            }

            if self.pos >= self.source.len()
                || self.looking_at(">")
                || self.looking_at("/>")
                || self.looking_at("</")
                || self.looking_at("<")
                || self.looking_at("{#")
                || self.looking_at("{:")
                || self.looking_at("{/")
            {
                break;
            }
            // {@ tags inside attributes: {@attach is an attribute, others break
            if self.looking_at("{@") && !self.looking_at_special_tag("attach") {
                break;
            }

            // Spread attribute: {...expr}
            if self.looking_at("{...") {
                let start = self.pos as u32;
                self.eat("{...")?;
                let expression_start = self.pos as u32;
                self.read_expression()?;
                let expression_span = Span::new(expression_start, self.pos as u32);
                self.eat("}")?;
                attributes.push(Attribute::Spread {
                    span: Span::new(start, self.pos as u32),
                });
                attribute_meta.push(AttributeMeta {
                    name_span: Span::new(start, start),
                    directive_subject_span: None,
                    value_span: None,
                    expression_span: Some(expression_span),
                    mustache_span: Some(Span::new(start, self.pos as u32)),
                    parts: Vec::new(),
                });
                continue;
            }

            // {@attach expr} attribute
            if self.looking_at_special_tag("attach") {
                let start = self.pos as u32;
                self.eat("{@attach")?;
                self.skip_whitespace();
                let expression_start = self.pos as u32;
                let expr = self.read_expression()?;
                let expression_span = Span::new(expression_start, self.pos as u32);
                self.eat("}")?;
                // Store as a Spread with a special marker (we'll detect it in serialization)
                // Using NormalAttribute with name "@attach"
                attributes.push(Attribute::NormalAttribute {
                    name: "@attach".to_string(),
                    value: AttributeValue::Expression(expr),
                    span: Span::new(start, self.pos as u32),
                });
                attribute_meta.push(AttributeMeta {
                    name_span: Span::new(start + 2, start + 9),
                    directive_subject_span: None,
                    value_span: Some(expression_span),
                    expression_span: Some(expression_span),
                    mustache_span: Some(Span::new(start, self.pos as u32)),
                    parts: Vec::new(),
                });
                continue;
            }

            // Shorthand attribute: {name}
            if self.looking_at("{") {
                let start = self.pos as u32;
                self.eat("{")?;
                let expression_start = self.pos as u32;
                let expr = self.read_expression()?;
                let expression_span = Span::new(expression_start, self.pos as u32);
                self.eat("}")?;
                if expr.trim().is_empty() {
                    self.report_error("Attribute shorthand cannot be empty");
                }
                attributes.push(Attribute::NormalAttribute {
                    name: expr.clone(),
                    value: AttributeValue::Expression(expr),
                    span: Span::new(start, self.pos as u32),
                });
                attribute_meta.push(AttributeMeta {
                    name_span: expression_span,
                    directive_subject_span: None,
                    value_span: Some(expression_span),
                    expression_span: Some(expression_span),
                    mustache_span: Some(Span::new(start, self.pos as u32)),
                    parts: Vec::new(),
                });
                continue;
            }

            // Named attribute or directive
            let attr_start = self.pos as u32;
            let attr_name_start = self.pos;
            while self.pos < self.source.len() {
                let ch = self.source.as_bytes()[self.pos];
                if ch.is_ascii_alphanumeric()
                    || ch == b'-'
                    || ch == b'_'
                    || ch == b':'
                    || ch == b'|'
                    || ch == b'.'
                {
                    self.pos += 1;
                } else {
                    break;
                }
            }
            let attr_name = &self.source[attr_name_start..self.pos];
            let name_end = self.pos;

            if attr_name.is_empty() {
                // Unexpected character — advance by full UTF-8 char length
                let ch_len = utf8_char_len(self.source.as_bytes()[self.pos]);
                self.pos += ch_len;
                continue;
            }

            // Check if this is a directive
            if let Some(directive) = parse_directive_name(attr_name) {
                // Check for value
                let directive_subject_end = attr_name
                    .find('|')
                    .map(|idx| attr_name_start + idx)
                    .unwrap_or(name_end);
                let directive_subject_span = attr_name.find(':').map(|colon| {
                    Span::new(
                        (attr_name_start + colon + 1) as u32,
                        directive_subject_end as u32,
                    )
                });
                let name_span = Span::new(attr_start, name_end as u32);
                if directive.1.is_empty() {
                    self.report_error(format!("`{attr_name}` name cannot be empty"));
                }
                self.skip_whitespace();
                if self.looking_at("=") {
                    self.eat("=")?;
                    self.skip_whitespace();
                    let parsed = self.parse_attribute_value()?;
                    if directive_value_is_invalid(&directive.0, &parsed.value) {
                        self.report_error(
                            "Directive value must be a JavaScript expression enclosed in curly braces",
                        );
                    }
                    attributes.push(Attribute::Directive {
                        kind: directive.0,
                        name: directive.1.to_string(),
                        modifiers: directive.2.iter().map(|s| s.to_string()).collect(),
                        value: parsed.value,
                        span: Span::new(attr_start, self.pos as u32),
                    });
                    attribute_meta.push(AttributeMeta {
                        name_span,
                        directive_subject_span,
                        ..parsed.meta
                    });
                } else {
                    // No value — span ends at the directive name end
                    attributes.push(Attribute::Directive {
                        kind: directive.0,
                        name: directive.1.to_string(),
                        modifiers: directive.2.iter().map(|s| s.to_string()).collect(),
                        value: AttributeValue::True,
                        span: Span::new(attr_start, name_end as u32),
                    });
                    attribute_meta.push(AttributeMeta {
                        name_span,
                        directive_subject_span,
                        value_span: None,
                        expression_span: None,
                        mustache_span: None,
                        parts: Vec::new(),
                    });
                }
                continue;
            }

            // Regular attribute — check for value
            self.skip_whitespace();
            let (value, mut meta) = if self.looking_at("=") {
                self.eat("=")?;
                self.skip_whitespace();
                let parsed = self.parse_attribute_value()?;
                (parsed.value, parsed.meta)
            } else {
                (
                    AttributeValue::True,
                    AttributeMeta {
                        name_span: Span::new(attr_name_start as u32, name_end as u32),
                        directive_subject_span: None,
                        value_span: None,
                        expression_span: None,
                        mustache_span: None,
                        parts: Vec::new(),
                    },
                )
            };
            meta.name_span = Span::new(attr_name_start as u32, name_end as u32);

            attributes.push(Attribute::NormalAttribute {
                name: attr_name.to_string(),
                value,
                span: Span::new(attr_start, self.pos as u32),
            });
            attribute_meta.push(meta);
        }

        Ok((attributes, attribute_meta))
    }

    fn report_duplicate_attributes(&mut self, attributes: &[Attribute]) {
        let mut seen = Vec::new();
        for attribute in attributes {
            let Some(key) = duplicate_attribute_key(attribute) else {
                continue;
            };
            if seen.iter().any(|seen_key| seen_key == &key) {
                self.report_error("Attributes need to be unique");
            } else {
                seen.push(key);
            }
        }
    }

    fn parse_attribute_value(&mut self) -> Result<ParsedAttributeValue, OxcDiagnostic> {
        if self.looking_at("\"") {
            self.eat("\"")?;
            let value_start = self.pos as u32;
            let value = self.eat_quoted_attr_value(b'"');
            let value_span = Span::new(value_start, self.pos as u32);
            self.eat("\"")?;
            // Check for embedded expressions
            if value.contains('{') {
                Ok(parse_concat_value(
                    &value,
                    value_start,
                    self.allocator,
                    &mut self.errors,
                ))
            } else {
                Ok(ParsedAttributeValue::new(
                    AttributeValue::Static(value.to_string()),
                    AttributeMeta {
                        name_span: Span::new(0, 0),
                        directive_subject_span: None,
                        value_span: Some(value_span),
                        expression_span: None,
                        mustache_span: None,
                        parts: Vec::new(),
                    },
                ))
            }
        } else if self.looking_at("'") {
            self.eat("'")?;
            let value_start = self.pos as u32;
            let value = self.eat_quoted_attr_value(b'\'');
            let value_span = Span::new(value_start, self.pos as u32);
            self.eat("'")?;
            if value.contains('{') {
                Ok(parse_concat_value(
                    &value,
                    value_start,
                    self.allocator,
                    &mut self.errors,
                ))
            } else {
                Ok(ParsedAttributeValue::new(
                    AttributeValue::Static(value.to_string()),
                    AttributeMeta {
                        name_span: Span::new(0, 0),
                        directive_subject_span: None,
                        value_span: Some(value_span),
                        expression_span: None,
                        mustache_span: None,
                        parts: Vec::new(),
                    },
                ))
            }
        } else {
            self.parse_unquoted_attribute_value()
        }
    }

    fn parse_unquoted_attribute_value(&mut self) -> Result<ParsedAttributeValue, OxcDiagnostic> {
        let value_start = self.pos;
        let mut static_start = self.pos;
        let mut parts = Vec::new();
        let mut part_meta = Vec::new();

        while self.pos < self.source.len() {
            if self.unquoted_attribute_value_done(value_start) {
                break;
            }

            if self.looking_at("{") {
                self.report_invalid_attribute_value_tag();
                if self.pos > static_start {
                    parts.push(AttributeValuePart::Static(
                        self.source[static_start..self.pos].to_string(),
                    ));
                    part_meta.push(AttributePartMeta {
                        span: Span::new(static_start as u32, self.pos as u32),
                        expression_span: None,
                        mustache_span: None,
                    });
                }

                let mustache_start = self.pos as u32;
                self.pos += 1;
                let expression_start = self.pos as u32;
                let expr = self.read_expression()?;
                let expression_span = Span::new(expression_start, self.pos as u32);
                if self.looking_at("}") {
                    self.pos += 1;
                } else {
                    self.report_error("Expected token }");
                }
                let mustache_span = Span::new(mustache_start, self.pos as u32);

                parts.push(AttributeValuePart::Expression(expr));
                part_meta.push(AttributePartMeta {
                    span: mustache_span,
                    expression_span: Some(expression_span),
                    mustache_span: Some(mustache_span),
                });
                static_start = self.pos;
            } else {
                self.pos += utf8_char_len(self.source.as_bytes()[self.pos]);
            }
        }

        if self.pos == value_start {
            self.report_error("Expected attribute value");
            return Ok(ParsedAttributeValue::new(
                AttributeValue::Static(String::new()),
                AttributeMeta {
                    name_span: Span::new(0, 0),
                    directive_subject_span: None,
                    value_span: Some(Span::new(value_start as u32, value_start as u32)),
                    expression_span: None,
                    mustache_span: None,
                    parts: Vec::new(),
                },
            ));
        }

        if static_start < self.pos {
            parts.push(AttributeValuePart::Static(
                self.source[static_start..self.pos].to_string(),
            ));
            part_meta.push(AttributePartMeta {
                span: Span::new(static_start as u32, self.pos as u32),
                expression_span: None,
                mustache_span: None,
            });
        }

        if parts.is_empty() {
            return Ok(ParsedAttributeValue::new(
                AttributeValue::Static(self.source[value_start..self.pos].to_string()),
                AttributeMeta {
                    name_span: Span::new(0, 0),
                    directive_subject_span: None,
                    value_span: Some(Span::new(value_start as u32, self.pos as u32)),
                    expression_span: None,
                    mustache_span: None,
                    parts: Vec::new(),
                },
            ));
        }

        if parts.len() == 1 {
            let meta = part_meta.remove(0);
            match parts.remove(0) {
                AttributeValuePart::Static(value) => {
                    return Ok(ParsedAttributeValue::new(
                        AttributeValue::Static(value),
                        AttributeMeta {
                            name_span: Span::new(0, 0),
                            directive_subject_span: None,
                            value_span: Some(meta.span),
                            expression_span: None,
                            mustache_span: None,
                            parts: Vec::new(),
                        },
                    ));
                }
                AttributeValuePart::Expression(expr) => {
                    let expression_span = meta.expression_span.unwrap_or(meta.span);
                    return Ok(ParsedAttributeValue::new(
                        AttributeValue::Expression(expr),
                        AttributeMeta {
                            name_span: Span::new(0, 0),
                            directive_subject_span: None,
                            value_span: Some(expression_span),
                            expression_span: Some(expression_span),
                            mustache_span: meta.mustache_span,
                            parts: Vec::new(),
                        },
                    ));
                }
            }
        }

        Ok(ParsedAttributeValue::new(
            AttributeValue::Concat(parts),
            AttributeMeta {
                name_span: Span::new(0, 0),
                directive_subject_span: None,
                value_span: Some(Span::new(value_start as u32, self.pos as u32)),
                expression_span: None,
                mustache_span: None,
                parts: part_meta,
            },
        ))
    }

    fn report_invalid_attribute_value_tag(&mut self) {
        report_invalid_attribute_value_tag(self.source, self.pos, &mut self.errors);
    }

    fn unquoted_attribute_value_done(&self, value_start: usize) -> bool {
        if self.looking_at("/>") && self.pos > value_start {
            return true;
        }

        matches!(
            self.source.as_bytes()[self.pos],
            b if b.is_ascii_whitespace()
                || matches!(b, b'"' | b'\'' | b'=' | b'<' | b'>' | b'`')
        )
    }

    fn eat_else_if_open(&mut self) -> Result<(), OxcDiagnostic> {
        self.eat("{:else")?;
        self.skip_whitespace();
        self.eat("if")
    }

    // ─── Block parsers ─────────────────────────────────────────────────

    fn parse_if_block(&mut self) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#if")?;
        self.skip_whitespace();
        let test_start = self.pos as u32;
        let test = self.read_expression()?;
        let test_span = Span::new(test_start, self.pos as u32);
        self.eat("}")?;
        let header_span = Span::new(start, self.pos as u32);

        // `{#if}` is a valid `<svelte:self>` ancestor position. The depth is
        // decremented in `finalize_if_chain` when the chain root is popped.
        self.svelte_self_allowed_depth += 1;
        let body_start = self.pos as u32;
        self.open_nodes.push(OpenNode::Block(OpenBlock::If {
            block_start: start,
            test,
            test_span,
            header_span,
            body_start,
            elseif: false,
            chained: false,
            consequent: None,
        }));
        self.enter_fragment();
        Ok(FragmentStep::Continue)
    }

    fn parse_each_block(&mut self) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#each")?;
        self.skip_whitespace();

        let header_start = self.pos as u32;
        let header = self.read_block_header();
        let header_span = Span::new(header_start, self.pos as u32);
        self.eat("}")?;

        let each_header = parse_each_header(header_span, header);
        if each_header_has_as_clause(header) && each_header.context.is_empty() {
            self.report_error(expected_pattern_message());
        }
        if each_header_has_empty_index_binding(header) {
            self.report_error(expected_identifier_message());
        }
        self.report_reserved_binding_identifier_diagnostic(&each_header.context);
        if let Some(index) = &each_header.index {
            self.report_reserved_binding_identifier_diagnostic(index);
        }

        // `{#each}` is a valid `<svelte:self>` ancestor position. The depth
        // is decremented in `finalize_top_open_block` when this entry is
        // popped (matching the pre-flatten
        // `parse_block_fragment_with_svelte_self_allowed` wrapper, which
        // wraps both the body and the fallback).
        self.svelte_self_allowed_depth += 1;
        let body_start = self.pos as u32;
        self.open_nodes.push(OpenNode::Block(OpenBlock::Each {
            block_start: start,
            expression: each_header.expression,
            expression_span: each_header.expression_span,
            context: each_header.context,
            context_span: each_header.context_span,
            index: each_header.index,
            index_span: each_header.index_span,
            key: each_header.key,
            key_span: each_header.key_span,
            header_span,
            active_fragment_start: body_start,
            body: None,
        }));
        self.enter_fragment();
        Ok(FragmentStep::Continue)
    }

    fn parse_await_block(&mut self) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#await")?;
        self.skip_whitespace();
        let header_start = self.pos as u32;
        let raw_header = self.read_block_header();
        let header_span = Span::new(header_start, self.pos as u32);
        let header = raw_header.trim().to_string();
        self.eat("}")?;
        let body_start = self.pos as u32;

        // Detect the shorthand forms `{#await expr then x}` and
        // `{#await expr catch x}` by scanning the header.
        let mut then_arm = AwaitArm::default();
        let mut catch_arm = AwaitArm::default();
        let mut active = AwaitArmKind::Pending;
        let expression;
        if let Some(then_pos) = scanner::find_top_level_spaced_word(&header, "then") {
            expression = header[..then_pos].trim().to_string();
            let (binding, binding_span) =
                header_binding_after_word(header_span, raw_header, "then");
            if !binding.is_empty() {
                self.report_reserved_binding_identifier_diagnostic(&binding);
                then_arm.binding = Some(binding);
                then_arm.binding_span = Some(binding_span);
            }
            active = AwaitArmKind::Then;
        } else if let Some(catch_pos) = scanner::find_top_level_spaced_word(&header, "catch") {
            expression = header[..catch_pos].trim().to_string();
            let (binding, binding_span) =
                header_binding_after_word(header_span, raw_header, "catch");
            if !binding.is_empty() {
                self.report_reserved_binding_identifier_diagnostic(&binding);
                catch_arm.binding = Some(binding);
                catch_arm.binding_span = Some(binding_span);
            }
            active = AwaitArmKind::Catch;
        } else {
            expression = header;
        }
        let expression_span = span_for_header_part(header_span, raw_header, &expression);

        self.open_nodes
            .push(OpenNode::Block(OpenBlock::Await(OpenAwaitBlock {
                block_start: start,
                expression,
                expression_span,
                pending: None,
                then_arm,
                catch_arm,
                active,
                active_fragment_start: body_start,
            })));
        self.enter_fragment();
        Ok(FragmentStep::Continue)
    }

    fn parse_key_block(&mut self) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#key")?;
        self.skip_whitespace();
        let expression_start = self.pos as u32;
        let expression = self.read_expression()?;
        let expression_span = Span::new(expression_start, self.pos as u32);
        self.eat("}")?;
        let body_start = self.pos as u32;
        self.open_nodes.push(OpenNode::Block(OpenBlock::Key {
            block_start: start,
            expression,
            expression_span,
            body_start,
        }));
        self.enter_fragment();
        Ok(FragmentStep::Continue)
    }

    fn parse_snippet_block(&mut self) -> Result<FragmentStep<'a>, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#snippet")?;
        self.skip_whitespace();

        let header_start = self.pos as u32;
        let header = self.read_block_header();
        let header_span = Span::new(header_start, self.pos as u32);
        self.eat("}")?;

        let snippet_header = parse_snippet_header(header_span, header);
        if snippet_header.name.is_empty() {
            self.report_error(expected_identifier_message());
        }
        self.report_reserved_binding_identifier_diagnostic(&snippet_header.name);
        if params_have_top_level_rest_parameter(&snippet_header.params) {
            self.report_error("Snippets do not support rest parameters; use an array instead");
        }

        // Snippets are valid `<svelte:self>` ancestor positions. The depth is
        // decremented in `finalize_top_open_block` when this entry is popped.
        self.svelte_self_allowed_depth += 1;
        let body_start = self.pos as u32;
        self.open_nodes.push(OpenNode::Block(OpenBlock::Snippet {
            block_start: start,
            name: snippet_header.name,
            name_span: snippet_header.name_span,
            type_params: snippet_header.type_params,
            type_params_span: snippet_header.type_params_span,
            params: snippet_header.params,
            params_span: snippet_header.params_span,
            body_start,
        }));
        self.enter_fragment();
        Ok(FragmentStep::Continue)
    }
}

// ─── Utility functions ─────────────────────────────────────────────────────

/// Return the byte length of the UTF-8 character starting at `byte`.
/// Falls back to 1 for continuation bytes (should not happen in valid UTF-8).
#[inline]
fn utf8_char_len(byte: u8) -> usize {
    if byte < 0xC0 {
        1
    }
    // continuation byte — shouldn't be a start
    else if byte < 0xE0 {
        2
    } else if byte < 0xF0 {
        3
    } else {
        4
    }
}

struct EachHeaderParts {
    expression: String,
    expression_span: Span,
    context: String,
    context_span: Span,
    index: Option<String>,
    index_span: Option<Span>,
    key: Option<String>,
    key_span: Option<Span>,
}

struct ParsedAttributeValue {
    value: AttributeValue,
    meta: AttributeMeta,
}

impl ParsedAttributeValue {
    fn new(value: AttributeValue, meta: AttributeMeta) -> Self {
        Self { value, meta }
    }
}

struct SnippetHeaderParts {
    name: String,
    name_span: Span,
    type_params: Option<String>,
    type_params_span: Option<Span>,
    params: String,
    params_span: Option<Span>,
}

fn parse_snippet_header(header_span: Span, header: &str) -> SnippetHeaderParts {
    let (prefix_end, params, params_span) =
        if let Some((paren_start, paren_end)) = scanner::find_trailing_top_level_parens(header) {
            (
                paren_start,
                header[paren_start + 1..paren_end].to_string(),
                Some(Span::new(
                    header_span.start + paren_start as u32 + 1,
                    header_span.start + paren_end as u32,
                )),
            )
        } else {
            (header.len(), String::new(), None)
        };

    let prefix = &header[..prefix_end];
    let prefix_start = prefix.len() - prefix.trim_start().len();
    let prefix_end = prefix.trim_end().len();
    let prefix_trimmed = &header[prefix_start..prefix_end];

    if prefix_trimmed.is_empty() {
        return SnippetHeaderParts {
            name: String::new(),
            name_span: Span::new(
                header_span.start + prefix_start as u32,
                header_span.start + prefix_start as u32,
            ),
            type_params: None,
            type_params_span: None,
            params,
            params_span,
        };
    }

    let generic_start = prefix_trimmed.find('<').map(|idx| prefix_start + idx);
    let generic_end = prefix_trimmed
        .ends_with('>')
        .then(|| prefix_start + prefix_trimmed.len() - 1);

    let (name_end, type_params, type_params_span) = match (generic_start, generic_end) {
        (Some(open), Some(close)) if close > open => (
            open,
            Some(header[open + 1..close].to_string()),
            Some(Span::new(
                header_span.start + open as u32 + 1,
                header_span.start + close as u32,
            )),
        ),
        _ => (prefix_end, None, None),
    };

    SnippetHeaderParts {
        name: header[prefix_start..name_end].trim().to_string(),
        name_span: span_for_trimmed_range(header_span, header, prefix_start, name_end),
        type_params,
        type_params_span,
        params,
        params_span,
    }
}

fn params_have_top_level_rest_parameter(params: &str) -> bool {
    let mut remaining = params;

    loop {
        let (part, next) = if let Some(comma) = scanner::find_top_level_comma(remaining) {
            (&remaining[..comma], Some(&remaining[comma + 1..]))
        } else {
            (remaining, None)
        };

        if part.trim_start().starts_with("...") {
            return true;
        }

        let Some(next) = next else {
            return false;
        };
        remaining = next;
    }
}

fn each_header_has_as_clause(raw_header: &str) -> bool {
    scanner::find_top_level_spaced_word(raw_header.trim(), "as").is_some()
}

fn each_header_has_empty_index_binding(raw_header: &str) -> bool {
    let Some(rest) = each_header_context_rest(raw_header) else {
        return false;
    };
    let rest_without_key =
        if let Some((paren_start, _)) = scanner::find_trailing_top_level_parens(rest) {
            &rest[..paren_start]
        } else {
            rest
        };

    scanner::find_top_level_comma(rest_without_key)
        .is_some_and(|comma| rest_without_key[comma + 1..].trim().is_empty())
}

fn each_header_context_rest(raw_header: &str) -> Option<&str> {
    let trimmed = raw_header.trim();
    let as_idx = scanner::find_top_level_spaced_word(trimmed, "as")?;
    let mut rest = trimmed[as_idx + "as".len()..].trim_start();

    if let Some(const_as_idx) = scanner::find_top_level_spaced_word(rest, "as") {
        if rest[..const_as_idx].trim() == "const" {
            rest = rest[const_as_idx + "as".len()..].trim_start();
        }
    }

    Some(rest)
}

/// Parse an `{#each}` header like `items as item, i (item.id)`.
fn parse_each_header(header_span: Span, raw_header: &str) -> EachHeaderParts {
    let trimmed = raw_header.trim();
    let trimmed_start = raw_header.len() - raw_header.trim_start().len();
    let trimmed_end = trimmed_start + trimmed.len();
    let header = &raw_header[trimmed_start..trimmed_end];
    let expression_only = || {
        let expression = header.trim().to_string();
        EachHeaderParts {
            expression,
            expression_span: span_for_trimmed_range(
                header_span,
                raw_header,
                trimmed_start,
                trimmed_end,
            ),
            context: String::new(),
            context_span: Span::new(
                header_span.start + trimmed_end as u32,
                header_span.start + trimmed_end as u32,
            ),
            index: None,
            index_span: None,
            key: None,
            key_span: None,
        }
    };

    let Some(as_idx) = scanner::find_top_level_spaced_word(header, "as") else {
        return expression_only();
    };

    let expression = header[..as_idx].trim().to_string();
    let expression_span = span_for_trimmed_range(
        header_span,
        raw_header,
        trimmed_start,
        trimmed_start + as_idx,
    );

    let mut rest_start = trimmed_start + as_idx + "as".len();
    let rest_limit = trimmed_start + header.len();
    let mut rest = &raw_header[rest_start..rest_limit];
    let leading = rest.len() - rest.trim_start().len();
    rest_start += leading;
    rest = &raw_header[rest_start..rest_limit];

    // Handle Svelte 5 `as const as context` by skipping the marker and using
    // the second `as` as the actual context boundary.
    if let Some(const_as_idx) = scanner::find_top_level_spaced_word(rest, "as") {
        if rest[..const_as_idx].trim() == "const" {
            rest_start += const_as_idx + "as".len();
            rest = &raw_header[rest_start..rest_limit];
            let leading = rest.len() - rest.trim_start().len();
            rest_start += leading;
            rest = &raw_header[rest_start..rest_limit];
        }
    }

    let (rest_without_key_start, rest_without_key_end, key, key_span) =
        if let Some((paren_start, paren_end)) = scanner::find_trailing_top_level_parens(rest) {
            let key_start = rest_start + paren_start + 1;
            let key_end = rest_start + paren_end;
            let key = raw_header[key_start..key_end].trim().to_string();
            let key_span = if key.is_empty() {
                None
            } else {
                Some(span_for_trimmed_range(
                    header_span,
                    raw_header,
                    key_start,
                    key_end,
                ))
            };
            (
                rest_start,
                rest_start + paren_start,
                key_span.as_ref().map(|_| key),
                key_span,
            )
        } else {
            (rest_start, rest_limit, None, None)
        };

    let rest_without_key = &raw_header[rest_without_key_start..rest_without_key_end];
    let (context, context_span, index, index_span) = if let Some(comma_idx) =
        scanner::find_top_level_comma(rest_without_key)
    {
        let context_start = rest_without_key_start;
        let context_end = rest_without_key_start + comma_idx;
        let index_start = rest_without_key_start + comma_idx + 1;
        let index_end = rest_without_key_end;
        let context = raw_header[context_start..context_end].trim().to_string();
        let index = raw_header[index_start..index_end].trim().to_string();
        (
            context,
            span_for_trimmed_range(header_span, raw_header, context_start, context_end),
            (!index.is_empty()).then_some(index),
            (!raw_header[index_start..index_end].trim().is_empty())
                .then(|| span_for_trimmed_range(header_span, raw_header, index_start, index_end)),
        )
    } else {
        let context = raw_header[rest_without_key_start..rest_without_key_end]
            .trim()
            .to_string();
        (
            context,
            span_for_trimmed_range(
                header_span,
                raw_header,
                rest_without_key_start,
                rest_without_key_end,
            ),
            None,
            None,
        )
    };

    EachHeaderParts {
        expression,
        expression_span,
        context,
        context_span,
        index,
        index_span,
        key,
        key_span,
    }
}

fn span_for_header_part(header_span: Span, header: &str, part: &str) -> Span {
    let leading = header.len() - header.trim_start().len();
    let rel_start = if part.is_empty() {
        leading
    } else {
        header.find(part).unwrap_or(leading)
    };
    Span::new(
        header_span.start + rel_start as u32,
        header_span.start + (rel_start + part.len()) as u32,
    )
}

fn span_for_trimmed_range(header_span: Span, header: &str, start: usize, end: usize) -> Span {
    let start = start.min(header.len());
    let end = end.min(header.len()).max(start);
    let segment = &header[start..end];
    let leading = segment.len() - segment.trim_start().len();
    let trailing = segment.len() - segment.trim_end().len();
    Span::new(
        header_span.start + (start + leading) as u32,
        header_span.start + (end - trailing) as u32,
    )
}

fn header_binding_after_word(header_span: Span, raw_header: &str, word: &str) -> (String, Span) {
    let Some(word_pos) = scanner::find_top_level_spaced_word(raw_header, word) else {
        let empty = Span::new(header_span.end, header_span.end);
        return (String::new(), empty);
    };
    let start = word_pos + word.len();
    let binding = raw_header[start..].trim().to_string();
    let span = span_for_trimmed_range(header_span, raw_header, start, raw_header.len());
    (binding, span)
}

fn parse_debug_identifiers(raw_span: Span, raw_identifiers: &str) -> (Vec<String>, Vec<Span>) {
    let mut identifiers = Vec::new();
    let mut spans = Vec::new();
    let mut part_start = 0usize;

    for (idx, ch) in raw_identifiers.char_indices() {
        if ch != ',' {
            continue;
        }
        push_debug_identifier(
            raw_span,
            raw_identifiers,
            part_start,
            idx,
            &mut identifiers,
            &mut spans,
        );
        part_start = idx + ch.len_utf8();
    }

    push_debug_identifier(
        raw_span,
        raw_identifiers,
        part_start,
        raw_identifiers.len(),
        &mut identifiers,
        &mut spans,
    );

    (identifiers, spans)
}

fn debug_tag_has_invalid_arguments(raw_identifiers: &str, allocator: &Allocator) -> bool {
    let trimmed = raw_identifiers.trim();
    if trimmed.is_empty() {
        return false;
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    if !parsed.errors.is_empty() {
        return false;
    }
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return false;
    };

    match strip_parentheses(expression) {
        oxc::ast::ast::Expression::Identifier(_) => false,
        oxc::ast::ast::Expression::SequenceExpression(sequence) => {
            sequence.expressions.iter().any(|expression| {
                !matches!(
                    strip_parentheses(expression),
                    oxc::ast::ast::Expression::Identifier(_)
                )
            })
        }
        _ => true,
    }
}

fn render_tag_expression_diagnostics(
    raw_expression: &str,
    allocator: &Allocator,
) -> Vec<&'static str> {
    use oxc::ast::ast::Argument;

    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    if !parsed.errors.is_empty() {
        return Vec::new();
    }
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return Vec::new();
    };

    let Some(call) = render_tag_call_expression(strip_parentheses(expression)) else {
        return vec!["`{@render ...}` tags can only contain call expressions"];
    };

    let mut diagnostics = Vec::new();
    if call
        .arguments
        .iter()
        .any(|argument| matches!(argument, Argument::SpreadElement(_)))
    {
        diagnostics.push("cannot use spread arguments in `{@render ...}` tags");
    }

    if render_tag_callee_uses_forbidden_helper(&call.callee) {
        diagnostics.push("Calling a snippet function using apply, bind or call is not allowed");
    }

    diagnostics
}

fn render_tag_call_expression<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
    use oxc::ast::ast::{ChainElement, Expression};

    match expression {
        Expression::CallExpression(call) => Some(call),
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::CallExpression(call) => Some(call),
            _ => None,
        },
        _ => None,
    }
}

fn render_tag_callee_uses_forbidden_helper(callee: &oxc::ast::ast::Expression<'_>) -> bool {
    use oxc::ast::ast::{ChainElement, Expression};

    match callee {
        Expression::StaticMemberExpression(member) => {
            matches!(member.property.name.as_str(), "apply" | "bind" | "call")
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(member) => {
                matches!(member.property.name.as_str(), "apply" | "bind" | "call")
            }
            ChainElement::CallExpression(call) => {
                render_tag_callee_uses_forbidden_helper(&call.callee)
            }
            _ => false,
        },
        _ => false,
    }
}

fn const_tag_declaration_diagnostic(raw_declaration: &str) -> Option<&'static str> {
    use oxc::allocator::Allocator;
    use oxc::ast::ast::Statement;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let wrapper = format!("const {raw_declaration}");
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &wrapper, SourceType::ts()).parse();

    let Some(Statement::VariableDeclaration(declaration)) = parsed.program.body.first() else {
        return Some("{@const ...} must consist of a single variable declaration");
    };
    let Some(first_declarator) = declaration.declarations.first() else {
        return Some("{@const ...} must consist of a single variable declaration");
    };
    if first_declarator.init.is_none() {
        return Some("Expected token =");
    }
    if declaration.declarations.len() != 1 {
        return Some("{@const ...} must consist of a single variable declaration");
    }

    None
}

fn push_debug_identifier(
    raw_span: Span,
    raw_identifiers: &str,
    start: usize,
    end: usize,
    identifiers: &mut Vec<String>,
    spans: &mut Vec<Span>,
) {
    let span = span_for_trimmed_range(raw_span, raw_identifiers, start, end);
    if span.start == span.end {
        return;
    }
    identifiers.push(raw_identifiers[start..end].trim().to_string());
    spans.push(span);
}

/// Parse a directive name like `on:click|preventDefault` into (kind, name, modifiers).
fn parse_directive_name(attr_name: &str) -> Option<(DirectiveKind, &str, Vec<&str>)> {
    let (prefix, rest) = attr_name.split_once(':')?;

    let kind = match prefix {
        "on" => DirectiveKind::EventHandler,
        "bind" => DirectiveKind::Binding,
        "class" => DirectiveKind::Class,
        "style" => DirectiveKind::StyleDirective,
        "use" => DirectiveKind::Use,
        "transition" => DirectiveKind::Transition,
        "in" => DirectiveKind::In,
        "out" => DirectiveKind::Out,
        "animate" => DirectiveKind::Animate,
        "let" => DirectiveKind::Let,
        _ => return None,
    };

    // Split name|modifier1|modifier2
    let parts: Vec<&str> = rest.split('|').collect();
    let name = parts[0];
    let modifiers = parts[1..].to_vec();

    Some((kind, name, modifiers))
}

fn directive_value_is_invalid(kind: &DirectiveKind, value: &AttributeValue) -> bool {
    if matches!(kind, DirectiveKind::StyleDirective) {
        return false;
    }

    match value {
        AttributeValue::True | AttributeValue::Expression(_) => false,
        AttributeValue::Concat(parts) => {
            !matches!(parts.as_slice(), [AttributeValuePart::Expression(_)])
        }
        AttributeValue::Static(_) => true,
    }
}

fn duplicate_attribute_key(attribute: &Attribute) -> Option<String> {
    match attribute {
        Attribute::NormalAttribute { name, .. } if name != "this" => {
            Some(format!("Attribute{name}"))
        }
        Attribute::Directive {
            kind: DirectiveKind::Binding,
            name,
            ..
        } if name != "this" => Some(format!("Attribute{name}")),
        Attribute::Directive {
            kind: DirectiveKind::Class,
            name,
            ..
        } if name != "this" => Some(format!("ClassDirective{name}")),
        Attribute::Directive {
            kind: DirectiveKind::StyleDirective,
            name,
            ..
        } if name != "this" => Some(format!("StyleDirective{name}")),
        _ => None,
    }
}

/// Parse a mustache's expression text directly as a single JS expression and
/// store the resulting node in the supplied allocator. Returns a reference
/// bound to the allocator's lifetime, so rules can read the typed
/// `Expression<'a>` without re-parsing.
///
/// Comment-sensitive rules still call
/// `crate::parser::expression::parse_template_expression` on the raw text —
/// that wrapper surfaces `ParserReturn.program.comments`, which
/// `parse_expression` alone doesn't expose.
fn parse_expr_into<'a>(
    allocator: &'a Allocator,
    text: &str,
) -> Option<&'a oxc::ast::ast::Expression<'a>> {
    use oxc::allocator::CloneIn;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Use the `void (EXPR);` wrapper so the whole expression must be
    // consumed — `parse_expression` alone silently accepts partial parses
    // (e.g. JSON-LD `{"@context": "..."}` parses as `StringLiteral("@context")`
    // with the rest discarded, producing false positives in rules that
    // treat the result as the full expression).
    let result = crate::parser::expression::parse_template_expression(trimmed, allocator);
    if !result.errors.is_empty() {
        return None;
    }
    let expr = crate::parser::expression::unwrap_template_expression(&result)?;
    // Clone into the allocator so the reference's lifetime is `'a` rather
    // than tied to the local `ParserReturn`.
    Some(allocator.alloc(expr.clone_in(allocator)))
}

/// Parse a concatenated attribute value like `"hello {name}!"`.
fn parse_concat_value(
    value: &str,
    value_start: u32,
    allocator: &Allocator,
    errors: &mut Vec<OxcDiagnostic>,
) -> ParsedAttributeValue {
    let mut parts = Vec::new();
    let mut part_meta = Vec::new();
    let mut parser = TemplateParser::new(value, allocator);
    let mut static_start = 0;

    while parser.pos < value.len() {
        if parser.looking_at("{") {
            report_invalid_attribute_value_tag(value, parser.pos, errors);
            if parser.pos > static_start {
                parts.push(AttributeValuePart::Static(
                    value[static_start..parser.pos].to_string(),
                ));
                part_meta.push(AttributePartMeta {
                    span: Span::new(
                        value_start + static_start as u32,
                        value_start + parser.pos as u32,
                    ),
                    expression_span: None,
                    mustache_span: None,
                });
            }
            let mustache_start = value_start + parser.pos as u32;
            parser.pos += 1;
            let expression_start = value_start + parser.pos as u32;
            let expr = parser.read_expression().unwrap_or_else(|_| {
                let rest = value[parser.pos..].to_string();
                parser.pos = value.len();
                rest
            });
            let expression_span = Span::new(expression_start, value_start + parser.pos as u32);
            if parser.looking_at("}") {
                parser.pos += 1;
            }
            let mustache_span = Span::new(mustache_start, value_start + parser.pos as u32);
            parts.push(AttributeValuePart::Expression(expr));
            part_meta.push(AttributePartMeta {
                span: mustache_span,
                expression_span: Some(expression_span),
                mustache_span: Some(mustache_span),
            });
            static_start = parser.pos;
        } else {
            parser.pos += utf8_char_len(value.as_bytes()[parser.pos]);
        }
    }

    if static_start < value.len() {
        parts.push(AttributeValuePart::Static(
            value[static_start..].to_string(),
        ));
        part_meta.push(AttributePartMeta {
            span: Span::new(
                value_start + static_start as u32,
                value_start + value.len() as u32,
            ),
            expression_span: None,
            mustache_span: None,
        });
    }

    ParsedAttributeValue::new(
        AttributeValue::Concat(parts),
        AttributeMeta {
            name_span: Span::new(0, 0),
            directive_subject_span: None,
            value_span: Some(Span::new(value_start, value_start + value.len() as u32)),
            expression_span: None,
            mustache_span: None,
            parts: part_meta,
        },
    )
}

fn report_invalid_attribute_value_tag(
    source: &str,
    brace_pos: usize,
    errors: &mut Vec<OxcDiagnostic>,
) {
    let Some(marker) = source.as_bytes().get(brace_pos + 1).copied() else {
        return;
    };

    if marker != b'#' && marker != b'@' {
        return;
    }

    let name_start = brace_pos + 2;
    let mut name_end = name_start;
    let bytes = source.as_bytes();
    while name_end < source.len() && bytes[name_end].is_ascii_lowercase() {
        name_end += 1;
    }

    let name = &source[name_start..name_end];
    if marker == b'#' {
        errors.push(OxcDiagnostic::error(format!(
            "{{#{name} ...}} block cannot be in attribute value"
        )));
    } else {
        errors.push(OxcDiagnostic::error(format!(
            "{{@{name} ...}} tag cannot be in attribute value"
        )));
    }
}

fn is_svelte_meta_tag(name: &str) -> bool {
    matches!(
        name,
        "svelte:head"
            | "svelte:options"
            | "svelte:window"
            | "svelte:document"
            | "svelte:body"
            | "svelte:element"
            | "svelte:component"
            | "svelte:self"
            | "svelte:fragment"
            | "svelte:boundary"
    )
}

fn is_root_only_svelte_meta_tag(name: &str) -> bool {
    matches!(
        name,
        "svelte:head" | "svelte:options" | "svelte:window" | "svelte:document" | "svelte:body"
    )
}

fn disallows_svelte_meta_children(name: &str) -> bool {
    matches!(
        name,
        "svelte:options" | "svelte:window" | "svelte:document" | "svelte:body"
    )
}

fn is_invalid_svelte_event_target_attribute(attribute: &Attribute) -> bool {
    match attribute {
        Attribute::Spread { .. } => true,
        Attribute::NormalAttribute { name, value, .. } => !is_event_attribute(name, value),
        Attribute::Directive { .. } => false,
    }
}

fn is_event_attribute(name: &str, value: &AttributeValue) -> bool {
    name.starts_with("on") && matches!(value, AttributeValue::Expression(_))
}

fn find_this_attribute_value(attributes: &[Attribute]) -> Option<&AttributeValue> {
    attributes.iter().find_map(|attribute| match attribute {
        Attribute::NormalAttribute { name, value, .. } if name == "this" => Some(value),
        _ => None,
    })
}

fn is_expression_attribute_value(value: &AttributeValue) -> bool {
    match value {
        AttributeValue::Expression(_) => true,
        AttributeValue::Concat(parts) => {
            matches!(parts.as_slice(), [AttributeValuePart::Expression(_)])
        }
        AttributeValue::Static(_) | AttributeValue::True => false,
    }
}

fn single_expression_attribute_text(value: &AttributeValue) -> Option<&str> {
    match value {
        AttributeValue::Expression(expression) => Some(expression.as_str()),
        AttributeValue::Concat(parts) => match parts.as_slice() {
            [AttributeValuePart::Expression(expression)] => Some(expression.as_str()),
            _ => None,
        },
        AttributeValue::Static(_) | AttributeValue::True => None,
    }
}

fn is_valid_svelte_fragment_attribute(attribute: &Attribute) -> bool {
    match attribute {
        Attribute::NormalAttribute { .. } => true,
        Attribute::Directive { kind, .. } => matches!(kind, DirectiveKind::Let),
        Attribute::Spread { .. } => false,
    }
}

fn is_valid_svelte_boundary_attribute_name(name: &str) -> bool {
    matches!(name, "onerror" | "failed" | "pending")
}

fn next_svelte_head_context(name: &str, current: bool) -> bool {
    if name == "svelte:head" {
        true
    } else if current && breaks_svelte_head_context(name) {
        false
    } else {
        current
    }
}

fn breaks_svelte_head_context(name: &str) -> bool {
    name != "slot" && !is_svelte_meta_tag(name) && is_element_or_component_name(name)
}

fn is_element_or_component_name(name: &str) -> bool {
    name.starts_with('!')
        || name
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first.is_uppercase())
}

fn is_valid_element_or_component_name(name: &str) -> bool {
    is_valid_element_name(name) || is_valid_component_name(name)
}

fn is_valid_element_name(name: &str) -> bool {
    is_doctype_name(name) || is_namespaced_element_name(name) || is_valid_html_tag_name(name)
}

fn is_doctype_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('!') else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn is_namespaced_element_name(name: &str) -> bool {
    let Some((prefix, local)) = name.split_once(':') else {
        return false;
    };
    if prefix.is_empty() || local.len() < 2 || local.ends_with('-') {
        return false;
    }
    is_ascii_alnum_name(prefix)
        && prefix.as_bytes()[0].is_ascii_alphabetic()
        && local.as_bytes()[0].is_ascii_alphabetic()
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn is_ascii_alnum_name(name: &str) -> bool {
    name.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_valid_html_tag_name(name: &str) -> bool {
    let mut chars = name.chars().peekable();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }

    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_alphanumeric() {
            chars.next();
        } else {
            break;
        }
    }

    if chars.peek().is_none() {
        return true;
    }
    if chars.next() != Some('-') {
        return false;
    }

    let mut has_custom_part = false;
    for ch in chars {
        if !is_potential_custom_element_name_char(ch) {
            return false;
        }
        has_custom_part = true;
    }
    has_custom_part
}

fn is_potential_custom_element_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '-'
                | '.'
                | '_'
                | '\u{00B7}'
                | '\u{00C0}'..='\u{00D6}'
                | '\u{00D8}'..='\u{00F6}'
                | '\u{00F8}'..='\u{037D}'
                | '\u{037F}'..='\u{1FFF}'
                | '\u{200C}'..='\u{200D}'
                | '\u{203F}'..='\u{2040}'
                | '\u{2070}'..='\u{218F}'
                | '\u{2C00}'..='\u{2FEF}'
                | '\u{3001}'..='\u{D7FF}'
                | '\u{F900}'..='\u{FDCF}'
                | '\u{FDF0}'..='\u{FFFD}'
                | '\u{10000}'..='\u{EFFFF}'
        )
}

fn is_valid_component_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if first.is_uppercase() {
        return chars.all(|ch| ch == '.' || is_component_name_continue(ch));
    }

    if !is_component_name_start(first) {
        return false;
    }

    let mut has_dot = false;
    let mut needs_member = false;
    for ch in chars {
        if needs_member {
            if is_component_name_continue(ch) {
                needs_member = false;
                continue;
            }
            return false;
        }
        if ch == '.' {
            has_dot = true;
            needs_member = true;
        } else if !is_component_name_continue(ch) {
            return false;
        }
    }

    has_dot && !needs_member
}

fn is_component_name_start(ch: char) -> bool {
    ch.is_alphabetic()
}

fn is_component_name_continue(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '$' | '\u{200C}' | '\u{200D}')
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotAncestorKind {
    Component,
    SvelteElement,
    CustomElement,
    RegularElement,
    Block,
    SnippetBlock,
}

impl SlotAncestorKind {
    fn is_slot_owner(self) -> bool {
        matches!(
            self,
            SlotAncestorKind::Component
                | SlotAncestorKind::SvelteElement
                | SlotAncestorKind::CustomElement
        )
    }
}

#[derive(Clone, Copy)]
struct SlotAncestor {
    kind: SlotAncestorKind,
}

#[derive(Clone, Copy)]
struct HtmlPlacementAncestor<'a> {
    name: &'a str,
    blocked_by_block: bool,
}

#[derive(Clone, Copy)]
struct EachMotionContext {
    keyed: bool,
    significant_body_children: usize,
}

#[derive(Default)]
struct SlotSnippetUsage {
    uses_render_tags: bool,
    uses_slots_identifier: bool,
    uses_slot_element: bool,
}

#[derive(Default)]
struct EventSyntaxUsage {
    first_event_directive_name: Option<String>,
    uses_event_attribute: bool,
}

impl SlotAncestor {
    fn block() -> Self {
        Self {
            kind: SlotAncestorKind::Block,
        }
    }

    fn snippet_block() -> Self {
        Self {
            kind: SlotAncestorKind::SnippetBlock,
        }
    }
}

fn slot_ancestor_kind(element: &Element<'_>) -> SlotAncestorKind {
    if is_component_slot_owner_name(&element.name) {
        SlotAncestorKind::Component
    } else if element.name == "svelte:element" {
        SlotAncestorKind::SvelteElement
    } else if is_custom_element_node(&element.name, &element.attributes) {
        SlotAncestorKind::CustomElement
    } else {
        SlotAncestorKind::RegularElement
    }
}

fn is_component_slot_owner_name(name: &str) -> bool {
    name == "svelte:component" || name == "svelte:self" || is_regular_component_element_name(name)
}

fn is_regular_element_for_attribute_validation(name: &str) -> bool {
    name != "slot"
        && !name.starts_with('!')
        && !name.starts_with("svelte:")
        && !is_regular_component_element_name(name)
}

fn uses_regular_element_attribute_rules(name: &str) -> bool {
    name == "svelte:element" || is_regular_element_for_attribute_validation(name)
}

fn uses_bind_target_rules(name: &str) -> bool {
    uses_regular_element_attribute_rules(name)
        || matches!(name, "svelte:window" | "svelte:document" | "svelte:body")
}

fn component_directive_is_invalid(kind: &DirectiveKind) -> bool {
    matches!(
        kind,
        DirectiveKind::Class
            | DirectiveKind::StyleDirective
            | DirectiveKind::Use
            | DirectiveKind::Transition
            | DirectiveKind::In
            | DirectiveKind::Out
            | DirectiveKind::Animate
    )
}

fn component_invalid_directive_message() -> &'static str {
    "This type of directive is not valid on components"
}

fn attribute_name_is_invalid(name: &str) -> bool {
    name.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit() || ch == '-' || ch == '.')
        || name.chars().any(|ch| {
            matches!(
                ch,
                '^' | '$'
                    | '@'
                    | '%'
                    | '&'
                    | '#'
                    | '?'
                    | '!'
                    | '|'
                    | '('
                    | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '*'
                    | '+'
                    | '~'
                    | ';'
            )
        })
}

fn is_valid_event_modifier(modifier: &str) -> bool {
    matches!(
        modifier,
        "preventDefault"
            | "stopPropagation"
            | "stopImmediatePropagation"
            | "capture"
            | "once"
            | "passive"
            | "nonpassive"
            | "self"
            | "trusted"
    )
}

fn event_handler_invalid_modifier_message() -> &'static str {
    "Valid event modifiers are preventDefault, stopPropagation, stopImmediatePropagation, capture, once, passive, nonpassive, self or trusted"
}

fn mixed_event_handler_syntax_message(name: &str) -> String {
    format!(
        "Mixing old (on:{name}) and new syntaxes for event handling is not allowed. Use only the on{name} syntax"
    )
}

fn invalid_arguments_usage_message() -> &'static str {
    "The arguments keyword cannot be used within the template or at the top level of a component"
}

fn experimental_async_message() -> &'static str {
    "Cannot use `await` in deriveds and template expressions, or at the top level of a component, unless the `experimental.async` compiler option is `true`"
}

fn unexpected_reserved_word_message(word: &str) -> String {
    format!("'{word}' is a reserved word in JavaScript and cannot be used here")
}

fn expected_pattern_message() -> &'static str {
    "Expected identifier or destructure pattern"
}

fn expected_identifier_message() -> &'static str {
    "Expected an identifier"
}

fn find_normal_attribute<'a>(attributes: &'a [Attribute], wanted: &str) -> Option<&'a Attribute> {
    attributes.iter().find(
        |attribute| matches!(attribute, Attribute::NormalAttribute { name, .. } if name == wanted),
    )
}

fn is_known_binding_property(name: &str) -> bool {
    KNOWN_BINDING_PROPERTIES.contains(&name)
}

fn binding_valid_elements(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "currentTime" | "duration" | "paused" | "buffered" | "seekable" | "played" | "volume"
        | "muted" | "playbackRate" | "seeking" | "ended" | "readyState" => {
            Some(&["audio", "video"])
        }
        "videoHeight" | "videoWidth" => Some(&["video"]),
        "naturalWidth" | "naturalHeight" => Some(&["img"]),
        "activeElement" | "fullscreenElement" | "pointerLockElement" | "visibilityState" => {
            Some(&["svelte:document"])
        }
        "innerWidth" | "innerHeight" | "outerWidth" | "outerHeight" | "scrollX" | "scrollY"
        | "online" | "devicePixelRatio" => Some(&["svelte:window"]),
        "indeterminate" | "checked" | "group" | "files" => Some(&["input"]),
        "open" => Some(&["details"]),
        "value" => Some(&["input", "textarea", "select"]),
        _ => None,
    }
}

fn binding_invalid_elements(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "clientWidth"
        | "clientHeight"
        | "offsetWidth"
        | "offsetHeight"
        | "contentRect"
        | "contentBoxSize"
        | "borderBoxSize"
        | "devicePixelContentBoxSize"
        | "innerText"
        | "innerHTML"
        | "textContent" => Some(&["svelte:window", "svelte:document"]),
        _ => None,
    }
}

fn possible_bindings_for_element(element_name: &str) -> Vec<&'static str> {
    let mut bindings = KNOWN_BINDING_PROPERTIES
        .iter()
        .copied()
        .filter(|binding| {
            binding_valid_elements(binding).map_or(true, |valid| valid.contains(&element_name))
                && binding_invalid_elements(binding)
                    .map_or(true, |invalid| !invalid.contains(&element_name))
        })
        .collect::<Vec<_>>();
    bindings.sort_unstable();
    bindings
}

fn binding_name_suggestion(name: &str, element_name: &str) -> Option<&'static str> {
    KNOWN_BINDING_PROPERTIES
        .iter()
        .copied()
        .filter(|binding| {
            binding_valid_elements(binding).map_or(true, |valid| valid.contains(&element_name))
        })
        .filter_map(|binding| {
            let score = levenshtein_similarity(name, binding);
            (score > 0.7).then_some((score, binding))
        })
        .max_by(|(left_score, left_binding), (right_score, right_binding)| {
            left_score
                .total_cmp(right_score)
                .then_with(|| right_binding.cmp(left_binding))
        })
        .map(|(_, binding)| binding)
}

fn levenshtein_similarity(left: &str, right: &str) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }

    let distance = levenshtein_distance(left, right);
    1.0 - distance as f64 / left.len().max(right.len()) as f64
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut previous = (0..=right.len()).collect::<Vec<_>>();
    let mut current = vec![0; right.len() + 1];

    for (left_index, left_byte) in left.iter().enumerate() {
        current[0] = left_index + 1;
        for (right_index, right_byte) in right.iter().enumerate() {
            let substitution = previous[right_index] + usize::from(left_byte != right_byte);
            let insertion = current[right_index] + 1;
            let deletion = previous[right_index + 1] + 1;
            current[right_index + 1] = substitution.min(insertion).min(deletion);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[right.len()]
}

fn is_svg_element_name(name: &str) -> bool {
    matches!(
        name,
        "altGlyph"
            | "altGlyphDef"
            | "altGlyphItem"
            | "animate"
            | "animateColor"
            | "animateMotion"
            | "animateTransform"
            | "circle"
            | "clipPath"
            | "color-profile"
            | "cursor"
            | "defs"
            | "desc"
            | "discard"
            | "ellipse"
            | "feBlend"
            | "feColorMatrix"
            | "feComponentTransfer"
            | "feComposite"
            | "feConvolveMatrix"
            | "feDiffuseLighting"
            | "feDisplacementMap"
            | "feDistantLight"
            | "feDropShadow"
            | "feFlood"
            | "feFuncA"
            | "feFuncB"
            | "feFuncG"
            | "feFuncR"
            | "feGaussianBlur"
            | "feImage"
            | "feMerge"
            | "feMergeNode"
            | "feMorphology"
            | "feOffset"
            | "fePointLight"
            | "feSpecularLighting"
            | "feSpotLight"
            | "feTile"
            | "feTurbulence"
            | "filter"
            | "font"
            | "font-face"
            | "font-face-format"
            | "font-face-name"
            | "font-face-src"
            | "font-face-uri"
            | "foreignObject"
            | "g"
            | "glyph"
            | "glyphRef"
            | "hatch"
            | "hatchpath"
            | "hkern"
            | "image"
            | "line"
            | "linearGradient"
            | "marker"
            | "mask"
            | "mesh"
            | "meshgradient"
            | "meshpatch"
            | "meshrow"
            | "metadata"
            | "missing-glyph"
            | "mpath"
            | "path"
            | "pattern"
            | "polygon"
            | "polyline"
            | "radialGradient"
            | "rect"
            | "set"
            | "solidcolor"
            | "stop"
            | "svg"
            | "switch"
            | "symbol"
            | "text"
            | "textPath"
            | "tref"
            | "tspan"
            | "unknown"
            | "use"
            | "view"
            | "vkern"
    )
}

const KNOWN_BINDING_PROPERTIES: &[&str] = &[
    "currentTime",
    "duration",
    "focused",
    "paused",
    "buffered",
    "seekable",
    "played",
    "volume",
    "muted",
    "playbackRate",
    "seeking",
    "ended",
    "readyState",
    "videoHeight",
    "videoWidth",
    "naturalWidth",
    "naturalHeight",
    "activeElement",
    "fullscreenElement",
    "pointerLockElement",
    "visibilityState",
    "innerWidth",
    "innerHeight",
    "outerWidth",
    "outerHeight",
    "scrollX",
    "scrollY",
    "online",
    "devicePixelRatio",
    "clientWidth",
    "clientHeight",
    "offsetWidth",
    "offsetHeight",
    "contentRect",
    "contentBoxSize",
    "borderBoxSize",
    "devicePixelContentBoxSize",
    "indeterminate",
    "checked",
    "group",
    "this",
    "innerText",
    "innerHTML",
    "textContent",
    "open",
    "value",
    "files",
];

fn is_contenteditable_binding(name: &str) -> bool {
    matches!(name, "innerText" | "innerHTML" | "textContent")
}

fn root_runes_option_enabled(nodes: &[TemplateNode<'_>]) -> bool {
    nodes.iter().any(|node| {
        let TemplateNode::Element(element) = node else {
            return false;
        };
        if element.name != "svelte:options" {
            return false;
        }

        element.attributes.iter().any(|attribute| {
            matches!(
                attribute,
                Attribute::NormalAttribute { name, value, .. }
                    if name == "runes" && static_option_bool(value) == Some(true)
            )
        })
    })
}

fn root_custom_element_option_enabled(nodes: &[TemplateNode<'_>]) -> bool {
    nodes.iter().any(|node| {
        let TemplateNode::Element(element) = node else {
            return false;
        };
        if element.name != "svelte:options" {
            return false;
        }

        element.attributes.iter().any(|attribute| {
            matches!(
                attribute,
                Attribute::NormalAttribute { name, value, .. }
                    if name == "customElement" && custom_element_option_value_enables(value)
            )
        })
    })
}

fn custom_element_option_value_enables(value: &AttributeValue) -> bool {
    match value {
        AttributeValue::Static(value) => !value.trim().is_empty(),
        AttributeValue::Expression(expression) => custom_element_expression_enables(expression),
        AttributeValue::Concat(parts) => match parts.as_slice() {
            [AttributeValuePart::Expression(expression)] => {
                custom_element_expression_enables(expression)
            }
            _ => false,
        },
        AttributeValue::True => false,
    }
}

fn custom_element_expression_enables(raw_expression: &str) -> bool {
    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return false;
    }

    let allocator = Allocator::default();
    let parsed = crate::parser::expression::parse_template_expression(trimmed, &allocator);
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return false;
    };

    matches!(
        strip_parentheses(expression),
        oxc::ast::ast::Expression::ObjectExpression(_)
    )
}

fn static_option_bool(value: &AttributeValue) -> Option<bool> {
    match value {
        AttributeValue::True => Some(true),
        AttributeValue::Expression(expression) => expression_static_option_bool(expression),
        AttributeValue::Concat(parts) => match parts.as_slice() {
            [AttributeValuePart::Expression(expression)] => {
                expression_static_option_bool(expression)
            }
            _ => None,
        },
        AttributeValue::Static(_) => None,
    }
}

fn expression_static_option_bool(expression: &str) -> Option<bool> {
    match expression.trim() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn uses_runes_attribute_value_rules(name: &str, attributes: &[Attribute]) -> bool {
    uses_regular_element_attribute_rules(name)
        || is_component_slot_owner_name(name)
        || is_custom_element_node(name, attributes)
}

fn walk_template_nodes<'node, 'a, F>(nodes: &'node [TemplateNode<'a>], visitor: &mut F)
where
    F: FnMut(&'node TemplateNode<'a>),
{
    for node in nodes {
        walk_template_node(node, visitor);
    }
}

fn walk_template_node<'node, 'a, F>(node: &'node TemplateNode<'a>, visitor: &mut F)
where
    F: FnMut(&'node TemplateNode<'a>),
{
    visitor(node);

    match node {
        TemplateNode::Element(element) => walk_template_nodes(&element.children, visitor),
        TemplateNode::IfBlock(block) => {
            walk_template_nodes(&block.consequent.nodes, visitor);
            if let Some(alternate) = &block.alternate {
                walk_template_node(alternate.as_ref(), visitor);
            }
        }
        TemplateNode::EachBlock(block) => {
            walk_template_nodes(&block.body.nodes, visitor);
            if let Some(fallback) = &block.fallback {
                walk_template_nodes(&fallback.nodes, visitor);
            }
        }
        TemplateNode::AwaitBlock(block) => {
            for fragment in await_block_fragments(block) {
                walk_template_nodes(&fragment.nodes, visitor);
            }
        }
        TemplateNode::KeyBlock(block) => walk_template_nodes(&block.body.nodes, visitor),
        TemplateNode::SnippetBlock(block) => walk_template_nodes(&block.body.nodes, visitor),
        TemplateNode::Text(_)
        | TemplateNode::MustacheTag(_)
        | TemplateNode::RawMustacheTag(_)
        | TemplateNode::DebugTag(_)
        | TemplateNode::ConstTag(_)
        | TemplateNode::RenderTag(_)
        | TemplateNode::Comment(_) => {}
    }
}

fn await_block_fragments<'node, 'a>(
    block: &'node AwaitBlock<'a>,
) -> impl Iterator<Item = &'node Fragment<'a>> {
    [
        block.pending.as_ref(),
        block.then.as_ref(),
        block.catch.as_ref(),
    ]
    .into_iter()
    .flatten()
}

fn collect_slot_snippet_usage(
    nodes: &[TemplateNode<'_>],
    usage: &mut SlotSnippetUsage,
    in_shadowroot_template: bool,
    allocator: &Allocator,
) {
    for node in nodes {
        collect_slot_snippet_node_usage(node, usage, in_shadowroot_template, allocator);
    }
}

fn collect_slot_snippet_node_usage(
    node: &TemplateNode<'_>,
    usage: &mut SlotSnippetUsage,
    in_shadowroot_template: bool,
    allocator: &Allocator,
) {
    match node {
        TemplateNode::Element(element) => {
            if element.name == "slot" && !in_shadowroot_template {
                usage.uses_slot_element = true;
            }
            for attribute in &element.attributes {
                if attribute_value_uses_slots_identifier(attribute_value(attribute), allocator) {
                    usage.uses_slots_identifier = true;
                }
            }

            let next_shadowroot_template = in_shadowroot_template
                || is_shadowroot_template_element(&element.name, &element.attributes);
            collect_slot_snippet_usage(
                &element.children,
                usage,
                next_shadowroot_template,
                allocator,
            );
        }
        TemplateNode::MustacheTag(tag) => {
            if expression_uses_slots_identifier(&tag.expression, allocator) {
                usage.uses_slots_identifier = true;
            }
        }
        TemplateNode::RawMustacheTag(tag) => {
            if expression_uses_slots_identifier(&tag.expression, allocator) {
                usage.uses_slots_identifier = true;
            }
        }
        TemplateNode::DebugTag(tag) => {
            if tag
                .identifiers
                .iter()
                .any(|identifier| identifier == "$$slots")
            {
                usage.uses_slots_identifier = true;
            }
        }
        TemplateNode::ConstTag(tag) => {
            if const_declaration_uses_slots_identifier(&tag.declaration) {
                usage.uses_slots_identifier = true;
            }
        }
        TemplateNode::RenderTag(tag) => {
            usage.uses_render_tags = true;
            if expression_uses_slots_identifier(&tag.expression, allocator) {
                usage.uses_slots_identifier = true;
            }
        }
        TemplateNode::IfBlock(block) => {
            if expression_uses_slots_identifier(&block.test, allocator) {
                usage.uses_slots_identifier = true;
            }
            collect_slot_snippet_usage(
                &block.consequent.nodes,
                usage,
                in_shadowroot_template,
                allocator,
            );
            if let Some(alternate) = &block.alternate {
                collect_slot_snippet_node_usage(
                    alternate,
                    usage,
                    in_shadowroot_template,
                    allocator,
                );
            }
        }
        TemplateNode::EachBlock(block) => {
            if expression_uses_slots_identifier(&block.expression, allocator)
                || block
                    .key
                    .as_ref()
                    .is_some_and(|key| expression_uses_slots_identifier(key, allocator))
            {
                usage.uses_slots_identifier = true;
            }
            collect_slot_snippet_usage(&block.body.nodes, usage, in_shadowroot_template, allocator);
            if let Some(fallback) = &block.fallback {
                collect_slot_snippet_usage(
                    &fallback.nodes,
                    usage,
                    in_shadowroot_template,
                    allocator,
                );
            }
        }
        TemplateNode::AwaitBlock(block) => {
            if expression_uses_slots_identifier(&block.expression, allocator) {
                usage.uses_slots_identifier = true;
            }
            for fragment in await_block_fragments(block) {
                collect_slot_snippet_usage(
                    &fragment.nodes,
                    usage,
                    in_shadowroot_template,
                    allocator,
                );
            }
        }
        TemplateNode::KeyBlock(block) => {
            if expression_uses_slots_identifier(&block.expression, allocator) {
                usage.uses_slots_identifier = true;
            }
            collect_slot_snippet_usage(&block.body.nodes, usage, in_shadowroot_template, allocator);
        }
        TemplateNode::SnippetBlock(block) => {
            collect_slot_snippet_usage(&block.body.nodes, usage, in_shadowroot_template, allocator);
        }
        TemplateNode::Text(_) | TemplateNode::Comment(_) => {}
    }
}

fn attribute_value(attribute: &Attribute) -> Option<&AttributeValue> {
    match attribute {
        Attribute::NormalAttribute { value, .. } | Attribute::Directive { value, .. } => {
            Some(value)
        }
        Attribute::Spread { .. } => None,
    }
}

fn attribute_value_uses_slots_identifier(
    value: Option<&AttributeValue>,
    allocator: &Allocator,
) -> bool {
    let Some(value) = value else {
        return false;
    };

    match value {
        AttributeValue::Expression(expression) => {
            expression_uses_slots_identifier(expression, allocator)
        }
        AttributeValue::Concat(parts) => parts.iter().any(|part| match part {
            AttributeValuePart::Expression(expression) => {
                expression_uses_slots_identifier(expression, allocator)
            }
            AttributeValuePart::Static(_) => false,
        }),
        AttributeValue::Static(_) | AttributeValue::True => false,
    }
}

fn expression_uses_slots_identifier(raw_expression: &str, allocator: &Allocator) -> bool {
    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return false;
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return false;
    };

    expression_contains_slots_identifier(expression)
}

fn const_declaration_uses_slots_identifier(raw_declaration: &str) -> bool {
    use oxc::ast_visit::Visit;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let wrapper = format!("const {raw_declaration}");
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &wrapper, SourceType::ts()).parse();
    let mut visitor = SlotsIdentifierVisitor { found: false };
    visitor.visit_program(&parsed.program);
    visitor.found
}

fn expression_contains_slots_identifier(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    use oxc::ast_visit::Visit;

    let mut visitor = SlotsIdentifierVisitor { found: false };
    visitor.visit_expression(expression);
    visitor.found
}

struct SlotsIdentifierVisitor {
    found: bool,
}

impl<'a> oxc::ast_visit::Visit<'a> for SlotsIdentifierVisitor {
    fn visit_identifier_reference(&mut self, it: &oxc::ast::ast::IdentifierReference<'a>) {
        if it.name == "$$slots" {
            self.found = true;
        }
    }
}

fn collect_event_syntax_usage(nodes: &[TemplateNode<'_>], usage: &mut EventSyntaxUsage) {
    walk_template_nodes(nodes, &mut |node| {
        if let TemplateNode::Element(element) = node {
            if uses_new_and_old_event_syntax_rules(&element.name) {
                collect_element_event_syntax_usage(&element.attributes, usage);
            }
        }
    });
}

fn collect_element_event_syntax_usage(attributes: &[Attribute], usage: &mut EventSyntaxUsage) {
    for attribute in attributes {
        match attribute {
            Attribute::Directive {
                kind: DirectiveKind::EventHandler,
                name,
                ..
            } => {
                usage
                    .first_event_directive_name
                    .get_or_insert_with(|| name.clone());
            }
            Attribute::NormalAttribute { name, value, .. }
                if is_event_attribute_name(name)
                    && single_expression_attribute_text(value).is_some() =>
            {
                usage.uses_event_attribute = true;
            }
            _ => {}
        }
    }
}

fn uses_new_and_old_event_syntax_rules(name: &str) -> bool {
    name == "svelte:element" || is_regular_element_for_attribute_validation(name)
}

fn is_event_attribute_name(name: &str) -> bool {
    name.starts_with("on")
}

fn attribute_value_has_invalid_arguments_usage(
    value: Option<&AttributeValue>,
    allocator: &Allocator,
) -> bool {
    let Some(value) = value else {
        return false;
    };

    match value {
        AttributeValue::Expression(expression) => {
            expression_text_has_invalid_arguments_usage(expression, allocator)
        }
        AttributeValue::Concat(parts) => parts.iter().any(|part| match part {
            AttributeValuePart::Expression(expression) => {
                expression_text_has_invalid_arguments_usage(expression, allocator)
            }
            AttributeValuePart::Static(_) => false,
        }),
        AttributeValue::Static(_) | AttributeValue::True => false,
    }
}

fn attribute_value_has_await_outside_functions(
    value: Option<&AttributeValue>,
    allocator: &Allocator,
) -> bool {
    let Some(value) = value else {
        return false;
    };

    match value {
        AttributeValue::Expression(expression) => {
            expression_text_has_await_outside_functions(expression, allocator)
        }
        AttributeValue::Concat(parts) => parts.iter().any(|part| match part {
            AttributeValuePart::Expression(expression) => {
                expression_text_has_await_outside_functions(expression, allocator)
            }
            AttributeValuePart::Static(_) => false,
        }),
        AttributeValue::Static(_) | AttributeValue::True => false,
    }
}

fn expression_text_has_invalid_arguments_usage(
    raw_expression: &str,
    allocator: &Allocator,
) -> bool {
    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return false;
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return false;
    };

    expression_contains_invalid_arguments_usage(expression)
}

fn const_declaration_has_invalid_arguments_usage(raw_declaration: &str) -> bool {
    use oxc::ast_visit::Visit;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let wrapper = format!("const {raw_declaration}");
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &wrapper, SourceType::ts()).parse();
    let mut visitor = InvalidArgumentsUsageVisitor { found: false };
    visitor.visit_program(&parsed.program);
    visitor.found
}

fn const_declaration_has_await_outside_functions(raw_declaration: &str) -> bool {
    use oxc::ast_visit::Visit;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    let wrapper = format!("const {raw_declaration}");
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, &wrapper, SourceType::ts()).parse();
    let mut visitor = AwaitExpressionVisitor { found: false };
    visitor.visit_program(&parsed.program);
    visitor.found
}

fn expression_contains_invalid_arguments_usage(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    use oxc::ast_visit::Visit;

    let mut visitor = InvalidArgumentsUsageVisitor { found: false };
    visitor.visit_expression(expression);
    visitor.found
}

struct InvalidArgumentsUsageVisitor {
    found: bool,
}

impl<'a> oxc::ast_visit::Visit<'a> for InvalidArgumentsUsageVisitor {
    fn visit_identifier_reference(&mut self, it: &oxc::ast::ast::IdentifierReference<'a>) {
        if it.name == "arguments" {
            self.found = true;
        }
    }

    fn visit_function(
        &mut self,
        _it: &oxc::ast::ast::Function<'a>,
        _flags: oxc::syntax::scope::ScopeFlags,
    ) {
    }
}

fn attribute_value_is_unquoted_sequence(
    source: &str,
    value: &AttributeValue,
    meta: &AttributeMeta,
) -> bool {
    matches!(value, AttributeValue::Concat(parts) if parts.len() > 1)
        && !attribute_value_is_quoted(source, meta)
}

fn attribute_value_is_quoted(source: &str, meta: &AttributeMeta) -> bool {
    let Some(value_span) = meta.value_span else {
        return false;
    };
    let Some(previous) = value_span
        .start
        .checked_sub(1)
        .and_then(|idx| source.as_bytes().get(idx as usize))
    else {
        return false;
    };

    matches!(previous, b'\'' | b'"')
}

fn attribute_value_is_unparenthesized_sequence_expression(
    value: &AttributeValue,
    allocator: &Allocator,
) -> bool {
    let Some(raw_expression) = single_expression_attribute_text(value) else {
        return false;
    };
    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return false;
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return false;
    };

    matches!(expression, oxc::ast::ast::Expression::SequenceExpression(_))
}

fn attribute_unquoted_sequence_message() -> &'static str {
    "Attribute values containing `{...}` must be enclosed in quote marks, unless the value only contains the expression"
}

fn attribute_invalid_sequence_expression_message() -> &'static str {
    "Comma-separated expressions are not allowed as attribute/directive values in runes mode, unless wrapped in parentheses"
}

fn bind_expression_diagnostics(
    raw_expression: &str,
    binding_name: &str,
    allocator: &Allocator,
) -> Vec<String> {
    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return Vec::new();
    };

    if parenthesized_sequence_expression(expression).is_some() {
        return vec![bind_invalid_parens_message(binding_name)];
    }

    let expression = strip_parentheses(expression);
    if let oxc::ast::ast::Expression::SequenceExpression(sequence) = expression {
        if binding_name == "group" {
            return vec![bind_group_invalid_expression_message().to_string()];
        }

        if sequence.expressions.len() != 2 {
            return vec![bind_invalid_expression_message().to_string()];
        }

        if sequence
            .expressions
            .iter()
            .any(binding_sequence_part_has_illegal_await)
        {
            return vec![illegal_await_expression_message().to_string()];
        }

        return Vec::new();
    }

    if !bind_expression_is_identifier_or_member(expression) {
        return vec![bind_invalid_expression_message().to_string()];
    }

    if expression_contains_await_outside_functions(expression) {
        return vec![illegal_await_expression_message().to_string()];
    }

    Vec::new()
}

fn parenthesized_sequence_expression<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
) -> Option<&'a oxc::ast::ast::SequenceExpression<'a>> {
    match expression {
        oxc::ast::ast::Expression::ParenthesizedExpression(parenthesized) => {
            match strip_parentheses(&parenthesized.expression) {
                oxc::ast::ast::Expression::SequenceExpression(sequence) => Some(sequence),
                _ => None,
            }
        }
        _ => None,
    }
}

fn binding_sequence_part_has_illegal_await(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    match strip_parentheses(expression) {
        oxc::ast::ast::Expression::ArrowFunctionExpression(arrow) => {
            function_body_contains_await_outside_nested_functions(&arrow.body)
        }
        expression => expression_contains_await_outside_functions(expression),
    }
}

fn bind_expression_is_identifier_or_member(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    matches!(
        expression,
        oxc::ast::ast::Expression::Identifier(_)
            | oxc::ast::ast::Expression::ComputedMemberExpression(_)
            | oxc::ast::ast::Expression::StaticMemberExpression(_)
            | oxc::ast::ast::Expression::PrivateFieldExpression(_)
    )
}

fn expression_text_has_await_outside_functions(
    raw_expression: &str,
    allocator: &Allocator,
) -> bool {
    let trimmed = raw_expression.trim();
    if trimmed.is_empty() {
        return false;
    }

    let parsed = crate::parser::expression::parse_template_expression(trimmed, allocator);
    let Some(expression) = crate::parser::expression::unwrap_template_expression(&parsed) else {
        return false;
    };

    expression_contains_await_outside_functions(strip_parentheses(expression))
}

fn expression_contains_await_outside_functions(expression: &oxc::ast::ast::Expression<'_>) -> bool {
    use oxc::ast_visit::Visit;

    let mut visitor = AwaitExpressionVisitor { found: false };
    visitor.visit_expression(expression);
    visitor.found
}

fn function_body_contains_await_outside_nested_functions(
    body: &oxc::ast::ast::FunctionBody<'_>,
) -> bool {
    use oxc::ast_visit::Visit;

    let mut visitor = AwaitExpressionVisitor { found: false };
    visitor.visit_function_body(body);
    visitor.found
}

struct AwaitExpressionVisitor {
    found: bool,
}

impl<'a> oxc::ast_visit::Visit<'a> for AwaitExpressionVisitor {
    fn visit_await_expression(&mut self, _it: &oxc::ast::ast::AwaitExpression<'a>) {
        self.found = true;
    }

    fn visit_function(
        &mut self,
        _it: &oxc::ast::ast::Function<'a>,
        _flags: oxc::syntax::scope::ScopeFlags,
    ) {
    }

    fn visit_arrow_function_expression(
        &mut self,
        _it: &oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) {
    }
}

fn bind_invalid_expression_message() -> &'static str {
    "Can only bind to an Identifier or MemberExpression or a `{get, set}` pair"
}

fn bind_group_invalid_expression_message() -> &'static str {
    "`bind:group` can only bind to an Identifier or MemberExpression"
}

fn bind_invalid_parens_message(binding_name: &str) -> String {
    format!("`bind:{binding_name}={{get, set}}` must not have surrounding parentheses")
}

fn illegal_await_expression_message() -> &'static str {
    "`use:`, `transition:` and `animate:` directives, attachments and bindings do not support await expressions"
}

fn motion_significant_child_count(nodes: &[TemplateNode<'_>]) -> usize {
    nodes
        .iter()
        .filter(|node| match node {
            TemplateNode::Comment(_) | TemplateNode::ConstTag(_) => false,
            TemplateNode::Text(text) if text.data.trim().is_empty() => false,
            _ => true,
        })
        .count()
}

fn transition_directive_type(attribute: &Attribute) -> Option<&'static str> {
    match attribute {
        Attribute::Directive {
            kind: DirectiveKind::Transition,
            ..
        } => Some("transition"),
        Attribute::Directive {
            kind: DirectiveKind::In,
            ..
        } => Some("in"),
        Attribute::Directive {
            kind: DirectiveKind::Out,
            ..
        } => Some("out"),
        _ => None,
    }
}

fn animation_invalid_placement_message() -> &'static str {
    "An element that uses the `animate:` directive must be the only child of a keyed `{#each ...}` block"
}

fn animation_missing_key_message() -> &'static str {
    "An element that uses the `animate:` directive must be the only child of a keyed `{#each ...}` block. Did you forget to add a key to your each block?"
}

fn is_custom_element_node(name: &str, attributes: &[Attribute]) -> bool {
    !name.starts_with("svelte:")
        && (name.contains('-')
            || attributes
                .iter()
                .any(|attribute| matches!(attribute, Attribute::NormalAttribute { name, .. } if name == "is")))
}

fn find_slot_attribute_value(attributes: &[Attribute]) -> Option<&AttributeValue> {
    attributes.iter().find_map(|attribute| match attribute {
        Attribute::NormalAttribute { name, value, .. } if name == "slot" => Some(value),
        _ => None,
    })
}

fn has_slot_attribute(attributes: &[Attribute]) -> bool {
    find_slot_attribute_value(attributes).is_some()
}

fn template_node_element<'node, 'a>(node: &'node TemplateNode<'a>) -> Option<&'node Element<'a>> {
    match node {
        TemplateNode::Element(element) => Some(element),
        _ => None,
    }
}

fn default_slot_child_is_allowed(node: &TemplateNode<'_>) -> bool {
    match node {
        TemplateNode::Text(text) if text.data.trim().is_empty() => true,
        TemplateNode::Element(element)
            if (is_regular_element_for_default_slot_check(&element.name)
                || element.name == "svelte:fragment")
                && has_slot_attribute(&element.attributes) =>
        {
            true
        }
        _ => false,
    }
}

fn component_has_attribute_or_binding(owner: &Element<'_>, name: &str) -> bool {
    owner.attributes.iter().any(|attribute| match attribute {
        Attribute::NormalAttribute {
            name: attribute_name,
            ..
        } => attribute_name == name,
        Attribute::Directive {
            kind: DirectiveKind::Binding,
            name: binding_name,
            ..
        } => binding_name == name,
        _ => false,
    })
}

fn component_has_implicit_children(children: &[TemplateNode<'_>]) -> bool {
    children.iter().any(|child| match child {
        TemplateNode::SnippetBlock(_) | TemplateNode::Comment(_) => false,
        TemplateNode::Text(text) if text.data.trim().is_empty() => false,
        _ => true,
    })
}

fn snippet_conflict_message() -> &'static str {
    "Cannot use explicit children snippet at the same time as implicit children content. Remove either the non-whitespace content or the children snippet block"
}

fn slot_snippet_conflict_message() -> &'static str {
    "Cannot use `<slot>` syntax and `{@render ...}` tags in the same component. Migrate towards `{@render ...}` tags completely"
}

fn is_regular_element_for_default_slot_check(name: &str) -> bool {
    name != "slot" && !name.starts_with("svelte:") && !is_regular_component_element_name(name)
}

fn slot_attribute_invalid_placement_message() -> &'static str {
    "Element with a slot='...' attribute must be a child of a component or a descendant of a custom element"
}

fn const_tag_allowed_in_element(element: &Element<'_>) -> bool {
    element.name == "svelte:fragment"
        || element.name == "svelte:boundary"
        || element.name == "svelte:component"
        || is_regular_component_element_name(&element.name)
        || ((element.name == "svelte:element"
            || is_regular_element_for_default_slot_check(&element.name))
            && has_slot_attribute(&element.attributes))
}

fn const_tag_invalid_placement_message() -> &'static str {
    "`{@const}` must be the immediate child of `{#snippet}`, `{#if}`, `{:else if}`, `{:else}`, `{#each}`, `{:then}`, `{:catch}`, `<svelte:fragment>`, `<svelte:boundary>` or `<Component>`"
}

fn text_placement_parent_for_element<'a>(
    element: &'a Element<'_>,
    current_parent: Option<&'a str>,
) -> Option<&'a str> {
    if element.name == "slot" || element.name == "svelte:boundary" {
        return current_parent;
    }
    if element.name.starts_with("svelte:") || is_regular_component_element_name(&element.name) {
        return None;
    }
    Some(element.name.as_str())
}

fn element_participates_in_html_placement(name: &str) -> bool {
    name != "slot"
        && !name.starts_with('!')
        && !name.starts_with("svelte:")
        && !is_regular_component_element_name(name)
}

fn html_placement_ancestors_blocked<'a>(
    ancestors: &[HtmlPlacementAncestor<'a>],
) -> Vec<HtmlPlacementAncestor<'a>> {
    ancestors
        .iter()
        .map(|ancestor| HtmlPlacementAncestor {
            name: ancestor.name,
            blocked_by_block: true,
        })
        .collect()
}

fn html_text_invalid_placement_message(parent: &str) -> Option<String> {
    let only = html_only_allowed_children(parent)?;
    Some(format!(
        "`<#text>` cannot be a child of `<{parent}>`. `<{parent}>` only allows these children: {}",
        html_tag_list(only)
    ))
}

fn html_tag_invalid_parent_message(child: &str, parent: &str) -> Option<String> {
    if child.contains('-') || parent.contains('-') || parent == "template" {
        return None;
    }

    if html_direct_disallowed_children(parent).is_some_and(|children| children.contains(&child)) {
        return Some(format!(
            "`<{child}>` cannot be a direct child of `<{parent}>`"
        ));
    }

    if html_descendant_disallowed_children(parent).is_some_and(|children| children.contains(&child))
    {
        return Some(format!("`<{child}>` cannot be a child of `<{parent}>`"));
    }

    if let Some(only) = html_only_allowed_children(parent) {
        if only.contains(&child) {
            return None;
        }
        return Some(format!(
            "`<{child}>` cannot be a child of `<{parent}>`. `<{parent}>` only allows these children: {}",
            html_tag_list(only)
        ));
    }

    match child {
        "body" | "caption" | "col" | "colgroup" | "frameset" | "frame" | "head" | "html" => {
            Some(format!("`<{child}>` cannot be a child of `<{parent}>`"))
        }
        "thead" | "tbody" | "tfoot" => Some(format!(
            "`<{child}>` must be the child of a `<table>`, not a `<{parent}>`"
        )),
        "td" | "th" => Some(format!(
            "`<{child}>` must be the child of a `<tr>`, not a `<{parent}>`"
        )),
        "tr" => Some(format!(
            "`<tr>` must be the child of a `<thead>`, `<tbody>`, or `<tfoot>`, not a `<{parent}>`"
        )),
        _ => None,
    }
}

fn html_tag_invalid_ancestor_message(
    child: &str,
    ancestors: &[HtmlPlacementAncestor<'_>],
    ancestor_index: usize,
) -> Option<String> {
    if child.contains('-') {
        return None;
    }

    let ancestor = ancestors.get(ancestor_index)?.name;
    if ancestor.contains('-') {
        return None;
    }

    let descendants = html_descendant_disallowed_children(ancestor)?;
    if !descendants.contains(&child) {
        return None;
    }

    if let Some(reset_by) = html_reset_by(ancestor) {
        if ancestors[..ancestor_index]
            .iter()
            .any(|candidate| candidate.name.contains('-') || reset_by.contains(&candidate.name))
        {
            return None;
        }
    }

    Some(format!(
        "`<{child}>` cannot be a descendant of `<{ancestor}>`"
    ))
}

fn html_direct_disallowed_children(parent: &str) -> Option<&'static [&'static str]> {
    match parent {
        "li" => Some(&["li"]),
        "thead" => Some(&["tbody", "tfoot"]),
        "tbody" => Some(&["tbody", "tfoot"]),
        "tfoot" => Some(&["tbody"]),
        "tr" => Some(&["tr", "tbody"]),
        "td" | "th" => Some(&["td", "th", "tr"]),
        _ => None,
    }
}

fn html_descendant_disallowed_children(parent: &str) -> Option<&'static [&'static str]> {
    match parent {
        "dt" | "dd" => Some(&["dt", "dd"]),
        "p" => Some(&[
            "address",
            "article",
            "aside",
            "blockquote",
            "div",
            "dl",
            "fieldset",
            "footer",
            "form",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "header",
            "hgroup",
            "hr",
            "main",
            "menu",
            "nav",
            "ol",
            "p",
            "pre",
            "section",
            "table",
            "ul",
        ]),
        "rt" | "rp" => Some(&["rt", "rp"]),
        "optgroup" => Some(&["optgroup"]),
        "option" => Some(&["option", "optgroup"]),
        "form" => Some(&["form"]),
        "a" => Some(&["a"]),
        "button" => Some(&["button"]),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => Some(&["h1", "h2", "h3", "h4", "h5", "h6"]),
        _ => None,
    }
}

fn html_reset_by(parent: &str) -> Option<&'static [&'static str]> {
    match parent {
        "dt" | "dd" => Some(&["dl"]),
        _ => None,
    }
}

fn html_only_allowed_children(parent: &str) -> Option<&'static [&'static str]> {
    match parent {
        "tr" => Some(&["th", "td", "style", "script", "template"]),
        "tbody" | "thead" | "tfoot" => Some(&["tr", "style", "script", "template"]),
        "colgroup" => Some(&["col", "template"]),
        "table" => Some(&[
            "caption", "colgroup", "tbody", "thead", "tfoot", "style", "script", "template",
        ]),
        "head" => Some(&[
            "base", "basefont", "bgsound", "link", "meta", "title", "noscript", "noframes",
            "style", "script", "template",
        ]),
        "html" => Some(&["head", "body", "frameset"]),
        "frameset" => Some(&["frame"]),
        _ => None,
    }
}

fn html_tag_list(tags: &[&str]) -> String {
    tags.iter()
        .map(|tag| format!("`<{tag}>`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn node_invalid_placement_message(message: String) -> String {
    format!("{message}. The browser will 'repair' the HTML (by moving, removing, or inserting elements) which breaks Svelte's assumptions about the structure of your components.")
}

enum StaticOptionValue {
    Bool,
    String(String),
    Null,
}

fn static_option_value(value: &AttributeValue) -> Option<StaticOptionValue> {
    match value {
        AttributeValue::True => Some(StaticOptionValue::Bool),
        AttributeValue::Static(value) => Some(StaticOptionValue::String(value.clone())),
        AttributeValue::Expression(expression) => expression_static_option_value(expression),
        AttributeValue::Concat(parts) => match parts.as_slice() {
            [AttributeValuePart::Static(value)] => Some(StaticOptionValue::String(value.clone())),
            [AttributeValuePart::Expression(expression)] => {
                expression_static_option_value(expression)
            }
            _ => None,
        },
    }
}

fn expression_static_option_value(expression: &str) -> Option<StaticOptionValue> {
    let trimmed = expression.trim();
    match trimmed {
        "true" | "false" => Some(StaticOptionValue::Bool),
        "null" => Some(StaticOptionValue::Null),
        _ => unquote_static_string_literal(trimmed).map(StaticOptionValue::String),
    }
}

fn unquote_static_string_literal(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let quote = *bytes.first()?;
    if (quote != b'\'' && quote != b'"') || bytes.last().copied() != Some(quote) {
        return None;
    }
    Some(value[1..value.len() - 1].to_string())
}

fn static_text_attribute_value(value: &AttributeValue) -> Option<&str> {
    match value {
        AttributeValue::Static(value) => Some(value.as_str()),
        _ => None,
    }
}

fn strip_parentheses<'a>(
    mut expression: &'a oxc::ast::ast::Expression<'a>,
) -> &'a oxc::ast::ast::Expression<'a> {
    while let oxc::ast::ast::Expression::ParenthesizedExpression(parenthesized) = expression {
        expression = &parenthesized.expression;
    }
    expression
}

fn custom_element_object_error(
    object: &oxc::ast::ast::ObjectExpression<'_>,
) -> Option<&'static str> {
    let mut tag = None;
    let mut props = None;
    let mut shadow = None;

    for property in &object.properties {
        let Some((name, value)) = object_property_identifier_value(property) else {
            return Some(custom_element_invalid_message());
        };
        match name {
            "tag" => tag = Some(value),
            "props" => props = Some(value),
            "shadow" => shadow = Some(value),
            "extend" => {}
            _ => {}
        }
    }

    if let Some(tag) = tag {
        let Some(tag) = expression_string_literal_value(strip_parentheses(tag)) else {
            return Some("Tag name must be lowercase and hyphenated");
        };
        if let Some(message) = custom_element_tag_error(tag) {
            return Some(message);
        }
    }

    if let Some(props) = props {
        let oxc::ast::ast::Expression::ObjectExpression(props) = strip_parentheses(props) else {
            return Some(custom_element_props_invalid_message());
        };
        if let Some(message) = custom_element_props_error(props) {
            return Some(message);
        }
    }

    if let Some(shadow) = shadow {
        match strip_parentheses(shadow) {
            oxc::ast::ast::Expression::StringLiteral(literal)
                if matches!(literal.value.as_str(), "open" | "none") => {}
            oxc::ast::ast::Expression::ObjectExpression(_) => {}
            _ => return Some(custom_element_shadow_invalid_message()),
        }
    }

    None
}

fn custom_element_props_error(props: &oxc::ast::ast::ObjectExpression<'_>) -> Option<&'static str> {
    for property in &props.properties {
        let Some((_name, value)) = object_property_identifier_value(property) else {
            return Some(custom_element_props_invalid_message());
        };
        let oxc::ast::ast::Expression::ObjectExpression(options) = strip_parentheses(value) else {
            return Some(custom_element_props_invalid_message());
        };

        for option in &options.properties {
            let Some((name, value)) = object_property_identifier_value(option) else {
                return Some(custom_element_props_invalid_message());
            };
            let value = strip_parentheses(value);
            match name {
                "type" => {
                    let Some(value) = expression_string_literal_value(value) else {
                        return Some(custom_element_props_invalid_message());
                    };
                    if !matches!(value, "String" | "Number" | "Boolean" | "Array" | "Object") {
                        return Some(custom_element_props_invalid_message());
                    }
                }
                "reflect" => {
                    if expression_boolean_literal_value(value).is_none() {
                        return Some(custom_element_props_invalid_message());
                    }
                }
                "attribute" => {
                    if expression_string_literal_value(value).is_none() {
                        return Some(custom_element_props_invalid_message());
                    }
                }
                _ => return Some(custom_element_props_invalid_message()),
            }
        }
    }

    None
}

fn object_property_identifier_value<'node, 'a>(
    property: &'node oxc::ast::ast::ObjectPropertyKind<'a>,
) -> Option<(&'node str, &'node oxc::ast::ast::Expression<'a>)> {
    let oxc::ast::ast::ObjectPropertyKind::ObjectProperty(property) = property else {
        return None;
    };
    if property.computed {
        return None;
    }
    let oxc::ast::ast::PropertyKey::StaticIdentifier(key) = &property.key else {
        return None;
    };
    Some((key.name.as_str(), &property.value))
}

fn expression_string_literal_value<'a>(
    expression: &'a oxc::ast::ast::Expression<'a>,
) -> Option<&'a str> {
    match expression {
        oxc::ast::ast::Expression::StringLiteral(literal) => Some(literal.value.as_str()),
        _ => None,
    }
}

fn expression_boolean_literal_value(expression: &oxc::ast::ast::Expression<'_>) -> Option<bool> {
    match expression {
        oxc::ast::ast::Expression::BooleanLiteral(literal) => Some(literal.value),
        _ => None,
    }
}

fn is_shadowroot_template_element(name: &str, attributes: &[Attribute]) -> bool {
    name == "template"
        && attributes.iter().any(|attribute| {
            matches!(attribute, Attribute::NormalAttribute { name, .. } if name == "shadowrootmode")
        })
}

fn is_valid_custom_element_tag(tag: &str) -> bool {
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return true;
    };
    first.is_ascii_lowercase()
        && tag.contains('-')
        && tag.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '-' | '_' | '.')
        })
}

fn is_reserved_custom_element_tag(tag: &str) -> bool {
    matches!(
        tag,
        "annotation-xml"
            | "color-profile"
            | "font-face"
            | "font-face-src"
            | "font-face-uri"
            | "font-face-format"
            | "font-face-name"
            | "missing-glyph"
    )
}

fn custom_element_tag_error(tag: &str) -> Option<&'static str> {
    if tag.is_empty() {
        None
    } else if !is_valid_custom_element_tag(tag) {
        Some("Tag name must be lowercase and hyphenated")
    } else if is_reserved_custom_element_tag(tag) {
        Some("Tag name is reserved")
    } else {
        None
    }
}

fn custom_element_invalid_message() -> &'static str {
    "\"customElement\" must be a string literal defining a valid custom element name or an object of the form { tag?: string; shadow?: \"open\" | \"none\" | `ShadowRootInit`; props?: { [key: string]: { attribute?: string; reflect?: boolean; type: .. } } }"
}

fn custom_element_props_invalid_message() -> &'static str {
    "\"props\" must be a statically analyzable object literal of the form \"{ [key: string]: { attribute?: string; reflect?: boolean; type?: \"String\" | \"Boolean\" | \"Number\" | \"Array\" | \"Object\" }\""
}

fn custom_element_shadow_invalid_message() -> &'static str {
    "\"shadow\" must be either \"open\", \"none\" or `ShadowRootInit` object."
}

fn is_component_like_element_name(name: &str) -> bool {
    name == "svelte:component"
        || name
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase())
        || name.contains('.')
}

fn is_regular_component_element_name(name: &str) -> bool {
    !name.starts_with("svelte:")
        && (name
            .chars()
            .next()
            .is_some_and(|first| first.is_uppercase())
            || name.contains('.'))
}

fn valid_svelte_meta_tag_list() -> &'static str {
    "svelte:head, svelte:options, svelte:window, svelte:document, svelte:body, svelte:element, svelte:component, svelte:self, svelte:fragment or svelte:boundary"
}

/// Check if opening a new element should implicitly close the parent.
fn should_implicitly_close(parent: &str, child: &str) -> bool {
    let parent_is = |name: &str| parent.eq_ignore_ascii_case(name);
    let child_is = |name: &str| child.eq_ignore_ascii_case(name);
    let child_is_any = |names: &[&str]| names.iter().any(|name| child_is(name));

    if parent_is("li") {
        child_is("li")
    } else if parent_is("dt") || parent_is("dd") {
        child_is_any(&["dt", "dd"])
    } else if parent_is("p") {
        child_is_any(&[
            "address",
            "article",
            "aside",
            "blockquote",
            "div",
            "dl",
            "fieldset",
            "footer",
            "form",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "header",
            "hgroup",
            "hr",
            "main",
            "menu",
            "nav",
            "ol",
            "p",
            "pre",
            "section",
            "table",
            "ul",
        ])
    } else if parent_is("rt") || parent_is("rp") {
        child_is_any(&["rt", "rp"])
    } else if parent_is("optgroup") {
        child_is("optgroup")
    } else if parent_is("option") {
        child_is_any(&["option", "optgroup"])
    } else if parent_is("thead") {
        child_is_any(&["tbody", "tfoot"])
    } else if parent_is("tbody") {
        child_is_any(&["tbody", "tfoot"])
    } else if parent_is("tfoot") {
        child_is("tbody")
    } else if parent_is("tr") {
        child_is_any(&["tr", "tbody"])
    } else if parent_is("td") || parent_is("th") {
        child_is_any(&["td", "th", "tr"])
    } else {
        false
    }
}

/// Check if an HTML element is a void element (self-closing by spec).
fn is_void_element(name: &str) -> bool {
    scanner::is_html_void_element(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text() {
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment("hello world", &alloc).unwrap();
        assert_eq!(result.nodes.len(), 1);
        match &result.nodes[0] {
            TemplateNode::Text(t) => assert_eq!(t.data, "hello world"),
            _ => panic!("expected Text node"),
        }
    }

    #[test]
    fn test_parse_element() {
        let source = "<div>hello</div>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        assert_eq!(result.nodes.len(), 1);
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "div");
                assert_eq!(el.children.len(), 1);
            }
            _ => panic!("expected Element node"),
        }
    }

    #[test]
    fn test_tag_name_tokenization_and_diagnostics_match_svelte() {
        let source = "<My$Comp /><foo@bar></foo@bar><comp.></comp.><Comp.></Comp.>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let names: Vec<&str> = fragment
            .nodes
            .iter()
            .filter_map(|node| match node {
                TemplateNode::Element(element) => Some(element.name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["My$Comp", "foo@bar", "comp.", "Comp."]);

        let invalid_name_errors = errors
            .iter()
            .filter(|error| {
                error.message.contains(
                    "Expected a valid element or component name. Components must have a valid variable name or dot notation expression",
                )
            })
            .count();
        assert_eq!(invalid_name_errors, 2, "{errors:?}");
    }

    #[test]
    fn test_parse_self_closing() {
        let source = "<br/>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "br");
                assert!(el.self_closing);
            }
            _ => panic!("expected Element node"),
        }
    }

    #[test]
    fn test_parse_mustache() {
        let source = "{count}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::MustacheTag(m) => assert_eq!(m.expression, "count"),
            _ => panic!("expected MustacheTag"),
        }
    }

    #[test]
    fn test_parse_mustache_with_comment_containing_brace() {
        let source = "{foo(/* } */ bar)}<span />";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        assert_eq!(result.nodes.len(), 2);
        match &result.nodes[0] {
            TemplateNode::MustacheTag(m) => assert_eq!(m.expression, "foo(/* } */ bar)"),
            _ => panic!("expected MustacheTag"),
        }
    }

    #[test]
    fn test_parse_mustache_with_regex_containing_brace() {
        let source = "{foo.match(/}/)}<span />";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        assert_eq!(result.nodes.len(), 2);
        match &result.nodes[0] {
            TemplateNode::MustacheTag(m) => assert_eq!(m.expression, "foo.match(/}/)"),
            _ => panic!("expected MustacheTag"),
        }
    }

    #[test]
    fn test_parse_if_block() {
        let source = "{#if visible}<p>hello</p>{/if}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::IfBlock(block) => {
                assert_eq!(block.test, "visible");
                assert_eq!(block.consequent.nodes.len(), 1);
            }
            _ => panic!("expected IfBlock"),
        }
    }

    #[test]
    fn test_parse_each_block() {
        let source = "{#each items as item, i (item.id)}<p>{item.name}</p>{/each}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::EachBlock(block) => {
                assert_eq!(block.expression, "items");
                assert_eq!(block.context, "item");
                assert_eq!(block.index.as_deref(), Some("i"));
                assert_eq!(block.key.as_deref(), Some("item.id"));
            }
            _ => panic!("expected EachBlock"),
        }
    }

    #[test]
    fn test_block_keyword_prefix_is_not_if_block() {
        let source = "{#ifx}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::MustacheTag(tag) => assert_eq!(tag.expression, "#ifx"),
            other => panic!("expected MustacheTag, got {other:?}"),
        }
    }

    #[test]
    fn test_svelte_keyword_prefix_reports_expected_whitespace() {
        let source = "{@htmlx value}{@renderx value}{@constx value}{@html}{@render}{@const}{#ifx}{#eachx}{#awaitx}{#keyx}{#snippetx}{#if}{#each}{#await}{#key}{#snippet}{#snippet()}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let expected_whitespace_errors = errors
            .iter()
            .filter(|error| error.to_string().contains("Expected whitespace"))
            .count();
        assert_eq!(expected_whitespace_errors, 17, "{errors:?}");
    }

    #[test]
    fn test_unknown_special_tags_report_expected_tag() {
        let source = "{@foo value}<div>{@attach value}</div><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let expected_tag_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Expected 'html', 'render', 'attach', 'const', or 'debug'")
            })
            .count();
        assert_eq!(expected_tag_errors, 2, "{errors:?}");
        assert_eq!(fragment.nodes.len(), 3);
        assert!(
            matches!(&fragment.nodes[2], TemplateNode::Element(element) if element.name == "p")
        );
    }

    #[test]
    fn test_each_header_ignores_typescript_as_inside_expression() {
        let source = "{#each items.map(x => x as Foo) as item}<p>{item}</p>{/each}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::EachBlock(block) => {
                assert_eq!(block.expression, "items.map(x => x as Foo)");
                assert_eq!(block.context, "item");
            }
            other => panic!("expected EachBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_else_if_allows_extra_whitespace_between_keywords() {
        let source = "{#if a}<p>a</p>{:else    if b}<p>b</p>{/if}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::IfBlock(block) => match block.alternate.as_deref() {
                Some(TemplateNode::IfBlock(else_if)) => assert_eq!(else_if.test, "b"),
                other => panic!("expected else-if block, got {other:?}"),
            },
            other => panic!("expected IfBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_if_invalid_continuation_reports_expected_token_and_recovers() {
        let source = "{#if a}<p>a</p>{:then}<p>x</p>{/if}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("Expected token {:else} or {:else if}")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_if_elseif_without_space_reports_diagnostic_and_recovers() {
        let source = "{#if a}<p>a</p>{:elseif b}<p>b</p>{/if}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("'elseif' should be 'else if'")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_unexpected_root_block_close_reports_diagnostic_and_recovers() {
        let source = "{/if}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("Unexpected block closing tag")));
        assert_eq!(fragment.nodes.len(), 1);
        match &fragment.nodes[0] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_unexpected_root_html_close_reports_diagnostic_and_recovers() {
        let source = "</div><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("attempted to close an element that was not open")));
        assert_eq!(fragment.nodes.len(), 1);
        match &fragment.nodes[0] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_void_element_close_reports_specific_diagnostic_and_recovers() {
        let source = "<br></br><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("Void elements cannot have children or closing tags")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_unclosed_element_reports_innermost_eof_frame_only() {
        let source = "<div><span>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let unclosed_errors: Vec<String> = errors
            .iter()
            .map(|error| error.to_string())
            .filter(|message| message.contains("was left open"))
            .collect();

        assert_eq!(unclosed_errors.len(), 1, "{errors:?}");
        assert!(unclosed_errors[0].contains("`<span>` was left open"));
    }

    #[test]
    fn test_unclosed_block_reports_innermost_eof_frame_only() {
        let source = "{#if ok}{#each items as item}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let unclosed_errors = errors
            .iter()
            .filter(|error| error.to_string().contains("Block was left open"))
            .count();

        assert_eq!(unclosed_errors, 1, "{errors:?}");
    }

    #[test]
    fn test_unclosed_element_inside_block_suppresses_parent_block_eof_error() {
        let source = "{#if ok}<div>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(
            errors
                .iter()
                .any(|error| error.to_string().contains("`<div>` was left open")),
            "{errors:?}"
        );
        assert!(
            !errors
                .iter()
                .any(|error| error.to_string().contains("Block was left open")),
            "{errors:?}"
        );
    }

    #[test]
    fn test_wrong_block_close_reports_expected_token_and_recovers() {
        let source = "{#if a}<p>a</p>{/each}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("Expected token if")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_attach_keyword_requires_boundary() {
        let source = "<div {@attachment value}></div>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => assert!(el.attributes.is_empty()),
            other => panic!("expected Element, got {other:?}"),
        }
    }

    #[test]
    fn test_duplicate_attribute_reports_diagnostic() {
        let source = r#"<div id="a" id="b"></div><p>after</p>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("Attributes need to be unique")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_bind_attribute_duplicates_normal_attribute() {
        let source = r#"<input value={value} bind:value={value} />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("Attributes need to be unique")));
    }

    #[test]
    fn test_class_directive_does_not_duplicate_normal_attribute() {
        let source = r#"<div class="base" class:active={active}></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.is_empty());
    }

    #[test]
    fn test_empty_attribute_shorthand_reports_diagnostic_and_recovers() {
        let source = "<div {}></div><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("Attribute shorthand cannot be empty")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.attributes.len(), 1);
                match &el.attributes[0] {
                    Attribute::NormalAttribute { name, .. } => assert!(name.is_empty()),
                    other => panic!("expected empty shorthand attribute, got {other:?}"),
                }
            }
            other => panic!("expected div element, got {other:?}"),
        }
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_missing_directive_name_reports_diagnostic_and_recovers() {
        let source = "<input bind: /><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("`bind:` name cannot be empty")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_directive_static_value_reports_diagnostic_except_style_directive() {
        let source = r#"<button on:click="handler"></button><div style:color="red"></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_value_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Directive value must be a JavaScript expression")
            })
            .count();
        assert_eq!(invalid_value_errors, 1);
    }

    #[test]
    fn test_block_and_special_tag_in_attribute_values_report_diagnostics() {
        let source = r#"<div title="a {#if x} b" data={@html x}></div><p>after</p>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("{#if ...} block cannot be in attribute value")));
        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("{@html ...} tag cannot be in attribute value")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_invalid_svelte_meta_tag_reports_diagnostic_and_recovers() {
        let source = "<svelte:foo /><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("Valid `<svelte:...>` tag names are")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_root_only_svelte_meta_duplicate_and_placement_diagnostics() {
        let source = "<svelte:head></svelte:head><div><svelte:head></svelte:head></div>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("A component can only have one `<svelte:head>` element")));
        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("`<svelte:head>` tags cannot be inside elements or blocks")));
    }

    #[test]
    fn test_svelte_head_attributes_report_diagnostics() {
        let source = "<svelte:head foo on:click={bar}></svelte:head>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let illegal_attribute_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<svelte:head>` cannot have attributes nor directives")
            })
            .count();
        assert_eq!(illegal_attribute_errors, 2);
    }

    #[test]
    fn test_svelte_event_target_attributes_report_diagnostics() {
        let source = r#"<svelte:window foo onresize={handler} onclick="handler" {...props} let:x></svelte:window><svelte:body class="x" let:y></svelte:body><svelte:document onclick={handler} let:z></svelte:document>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let illegal_attribute_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("does not support non-event attributes or spread attributes")
            })
            .count();
        assert_eq!(illegal_attribute_errors, 4);

        let invalid_let_directives = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`let:` directive at invalid position")
            })
            .count();
        assert_eq!(invalid_let_directives, 3);
    }

    #[test]
    fn test_childless_svelte_meta_tags_report_diagnostics() {
        let source = "<svelte:options><p /></svelte:options><svelte:window>text</svelte:window>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_content_errors = errors
            .iter()
            .filter(|error| error.to_string().contains("cannot have children"))
            .count();
        assert_eq!(invalid_content_errors, 2);
    }

    #[test]
    fn test_svelte_component_this_attribute_diagnostics() {
        let source = r#"<svelte:component /><svelte:component this="Foo" /><svelte:component this={Foo} /><svelte:component this="{Bar}" />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("`<svelte:component>` must have a 'this' attribute")));
        let invalid_this_errors = errors
            .iter()
            .filter(|error| error.to_string().contains("Invalid component definition"))
            .count();
        assert_eq!(invalid_this_errors, 1);
    }

    #[test]
    fn test_svelte_element_this_attribute_diagnostics() {
        let source = r#"<svelte:element /><svelte:element this /><svelte:element this="div" />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let missing_this_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<svelte:element>` must have a 'this' attribute with a value")
            })
            .count();
        assert_eq!(missing_this_errors, 2);
    }

    #[test]
    fn test_svelte_fragment_placement_diagnostics() {
        let source = "<svelte:fragment></svelte:fragment><div><svelte:fragment /></div><Component><svelte:fragment /></Component>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let placement_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<svelte:fragment>` must be the direct child of a component")
            })
            .count();
        assert_eq!(placement_errors, 2);
    }

    #[test]
    fn test_svelte_fragment_attribute_diagnostics() {
        let source = r#"<Component><svelte:fragment slot="x" let:item foo on:click={handle} {...props}></svelte:fragment></Component>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let attribute_errors = errors
            .iter()
            .filter(|error| {
                error.to_string().contains(
                    "`<svelte:fragment>` can only have a slot attribute and (optionally) a let: directive",
                )
            })
            .count();
        assert_eq!(attribute_errors, 2);
    }

    #[test]
    fn test_svelte_self_valid_placements() {
        let source = r#"{#if ok}<svelte:self />{/if}{#each items as item}<svelte:self />{/each}{#snippet row()}<svelte:self />{/snippet}<Component><div><svelte:self /></div></Component>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(
            !errors.iter().any(|error| error
                .to_string()
                .contains("`<svelte:self>` components can only exist")),
            "{errors:?}"
        );
    }

    #[test]
    fn test_svelte_self_invalid_placements() {
        let source = r#"<svelte:self />{#await promise}<svelte:self />{/await}{#key value}<svelte:self />{/key}<svelte:component this={Component}><svelte:self /></svelte:component>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_placement_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<svelte:self>` components can only exist")
            })
            .count();
        assert_eq!(invalid_placement_errors, 4);
    }

    #[test]
    fn test_slot_element_attribute_diagnostics() {
        let source = r#"<slot name={dynamic}></slot><slot name="default"></slot><slot on:click={handle}></slot><slot {...props} let:item title="ok"></slot>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        for message in [
            "slot attribute must be a static value",
            "`default` is a reserved word — it cannot be used as a slot name",
            "`<slot>` can only receive attributes and (optionally) let directives",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(message)),
                "missing diagnostic: {message}; got {errors:?}"
            );
        }
    }

    #[test]
    fn test_shadowroot_slot_does_not_use_svelte_slot_diagnostics() {
        let source = r#"<template shadowrootmode="open"><slot name={dynamic} on:click={handle}></slot></template>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(
            !errors.iter().any(|error| {
                let message = error.to_string();
                message.contains("slot attribute must be a static value")
                    || message.contains("`<slot>` can only receive attributes")
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn test_slot_attribute_diagnostics() {
        let source = r#"<div slot="root"></div><Parent><div slot={dynamic}></div><span slot="x"></span><p slot="x"></p>text<div slot="default"></div>{#if ok}<b slot="nested"></b>{/if}</Parent>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        for message in [
            "Element with a slot='...' attribute must be a child of a component or a descendant of a custom element",
            "slot attribute must be a static value",
            "Duplicate slot name 'x' in <Parent>",
            "Found default slot content alongside an explicit slot=\"default\"",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(message)),
                "missing diagnostic: {message}; got {errors:?}"
            );
        }
    }

    #[test]
    fn test_slot_attribute_valid_contexts() {
        let source = r#"<Child slot={dynamic} /><Parent>{#if ok}<Child slot={dynamic} />{/if}<my-el><span slot={dynamic}></span></my-el><slot slot={dynamic}></slot><div slot="x"></div><svelte:fragment slot="y"></svelte:fragment></Parent>{#snippet row()}<Child slot="snippet" />{/snippet}"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn test_component_snippet_child_diagnostics() {
        let source = r#"<Component foo bind:bar>{#snippet foo()}{/snippet}{#snippet bar()}{/snippet}</Component><Component>text{#snippet children()}{/snippet}</Component><Component><!--ok-->{#snippet children()}{/snippet}</Component><Component>   {#snippet children()}{/snippet}</Component><Component>{#if ok}{#snippet children()}{/snippet}{/if}</Component>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("This snippet is shadowing the prop"))
                .count(),
            2,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(snippet_conflict_message()))
                .count(),
            1,
            "{errors:?}"
        );
    }

    #[test]
    fn test_slot_snippet_conflict_diagnostics() {
        let alloc = oxc::allocator::Allocator::default();

        for source in [
            r#"<slot />{@render children?.()}"#,
            r#"{@render children?.()}{$$slots.default}"#,
            r#"{@render children?.()}<div data-x={$$slots.default}></div>"#,
            r#"{@render children?.()}{#if ok}{@const x = $$slots.default}{/if}"#,
            r#"{@render children?.()}{@debug $$slots}"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert_eq!(
                errors
                    .iter()
                    .filter(|error| error.to_string().contains(slot_snippet_conflict_message()))
                    .count(),
                1,
                "{source}: {errors:?}"
            );
        }

        for source in [
            r#"<svelte:options customElement="x-foo" /><slot />{@render children?.()}"#,
            r#"<template shadowrootmode="open"><slot /></template>{@render children?.()}"#,
            r#"{@render children?.()}<div>{"$$slots"}</div>"#,
            r#"{@render children?.()}{#if ok}{@const x = "$$slots"}{/if}"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert!(
                !errors
                    .iter()
                    .any(|error| error.to_string().contains(slot_snippet_conflict_message())),
                "{source}: {errors:?}"
            );
        }
    }

    #[test]
    fn test_svelte_fragment_is_invalid_under_svelte_self() {
        let source = r#"{#if ok}<svelte:self><svelte:fragment slot="x"></svelte:fragment></svelte:self>{/if}"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(
            errors.iter().any(|error| error
                .to_string()
                .contains("`<svelte:fragment>` must be the direct child of a component")),
            "{errors:?}"
        );
    }

    #[test]
    fn test_svelte_boundary_attribute_diagnostics() {
        let source = r#"<svelte:boundary onerror={handler} failed={fallback} pending={pending}></svelte:boundary><svelte:boundary foo={bar} on:click={handle} {...props} failed="fallback" pending></svelte:boundary>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_attribute_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Valid attributes on `<svelte:boundary>` are `onerror` and `failed`")
            })
            .count();
        assert_eq!(invalid_attribute_errors, 3);

        let invalid_value_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Attribute value must be a non-string expression")
            })
            .count();
        assert_eq!(invalid_value_errors, 2);
    }

    #[test]
    fn test_svelte_options_valid_static_attributes() {
        let source = r#"<svelte:options customElement="my-widget" runes={true} immutable accessors={false} preserveWhitespace={true} namespace="svg" css="injected" />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn test_svelte_options_valid_custom_element_object() {
        let source = r#"<svelte:options customElement={{ tag: "my-widget", shadow: { mode: "closed" }, props: { count: { type: "Number", reflect: true, attribute: "count" }, label: {} }, extend, unknown: value }} />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.is_empty(), "{errors:?}");
    }

    #[test]
    fn test_svelte_options_attribute_diagnostics() {
        let source = r#"<svelte:options on:click={handle} {...props} foo runes="true" namespace="bad" css="external" tag="x-foo" customElement="Foo" /><svelte:options customElement="font-face" />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_static_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<svelte:options>` can only receive static attributes")
            })
            .count();
        assert_eq!(invalid_static_errors, 2);

        for message in [
            "`<svelte:options>` unknown attribute 'foo'",
            "Value must be true or false, if specified",
            "Value must be \"html\", \"mathml\" or \"svg\", if specified",
            "Value must be \"injected\", if specified",
            "\"tag\" option is deprecated",
            "Tag name must be lowercase and hyphenated",
            "Tag name is reserved",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(message)),
                "missing diagnostic: {message}; got {errors:?}"
            );
        }
    }

    #[test]
    fn test_svelte_options_custom_element_object_diagnostics() {
        let source = r#"<svelte:options customElement={"my-widget"} /><svelte:options customElement={{ tag }} /><svelte:options customElement={{ [tag]: "my-widget" }} /><svelte:options customElement={{ props: foo }} /><svelte:options customElement={{ props: { "count": { type: "Number" } } }} /><svelte:options customElement={{ props: { count: { type: "Date" } } }} /><svelte:options customElement={{ props: { count: { reflect: "true" } } }} /><svelte:options customElement={{ props: { count: { attribute: true } } }} /><svelte:options customElement={{ shadow: "closed" }} />"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        for message in [
            custom_element_invalid_message(),
            "Tag name must be lowercase and hyphenated",
            custom_element_props_invalid_message(),
            custom_element_shadow_invalid_message(),
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(message)),
                "missing diagnostic: {message}; got {errors:?}"
            );
        }
    }

    #[test]
    fn test_await_shorthand_header_handles_object_literal() {
        let source = "{#await foo({ a: 1 }) then value}<p>{value}</p>{/await}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::AwaitBlock(block) => {
                assert_eq!(block.expression, "foo({ a: 1 })");
                assert_eq!(block.then_binding.as_deref(), Some("value"));
                assert!(block.then.is_some());
                assert!(block.pending.is_none());
            }
            other => panic!("expected AwaitBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_each_invalid_continuation_reports_expected_token_and_recovers() {
        let source = "{#each items as item}<p>{item}</p>{:then}<p>x</p>{/each}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("Expected token {:else}")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_await_then_binding_handles_destructuring() {
        let source = "{#await promise}<p>pending</p>{:then { value }}<p>{value}</p>{/await}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::AwaitBlock(block) => {
                assert_eq!(block.expression, "promise");
                assert_eq!(block.then_binding.as_deref(), Some("{ value }"));
                assert!(block.pending.is_some());
                assert!(block.then.is_some());
            }
            other => panic!("expected AwaitBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_await_shorthand_fragment_span_is_monotonic() {
        let source = "{#await promise then value}<p>{value}</p>{/await}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::AwaitBlock(block) => {
                let then = block.then.as_ref().expect("expected then fragment");
                assert!(then.span.start <= then.span.end);
            }
            other => panic!("expected AwaitBlock, got {other:?}"),
        }
    }

    #[test]
    fn test_await_duplicate_then_reports_diagnostic_and_recovers() {
        let source =
            "{#await p}<p>pending</p>{:then a}<p>a</p>{:then b}<p>b</p>{/await}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("{:then} cannot appear more than once within a block")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_await_invalid_continuation_reports_expected_token_and_recovers() {
        let source = "{#await p}<p>pending</p>{:else}<p>x</p>{/await}<p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("Expected token {:then ...} or {:catch ...}")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_title_with_multibyte_utf8() {
        // Regression test: multi-byte UTF-8 chars in <title> should not split
        // text at invalid byte boundaries.
        let source = "<title>{name} \u{2022} {site}</title>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "title");
                // Should have 3 children: mustache, text with bullet, mustache
                assert_eq!(el.children.len(), 3);
                match &el.children[1] {
                    TemplateNode::Text(t) => assert!(t.data.contains('\u{2022}')),
                    other => panic!("expected Text, got {:?}", other),
                }
            }
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn test_title_uses_normal_fragment_parser() {
        let source = "<title>{#if visible}<b>{@html value}</b>{/if}</title>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => match &el.children[0] {
                TemplateNode::IfBlock(block) => {
                    assert_eq!(block.test, "visible");
                    assert_eq!(block.consequent.nodes.len(), 1);
                    match &block.consequent.nodes[0] {
                        TemplateNode::Element(child) => {
                            assert_eq!(child.name, "b");
                            assert!(matches!(
                                &child.children[0],
                                TemplateNode::RawMustacheTag(tag) if tag.expression == "value"
                            ));
                        }
                        other => panic!("expected child element, got {other:?}"),
                    }
                }
                other => panic!("expected IfBlock child, got {other:?}"),
            },
            other => panic!("expected title element, got {other:?}"),
        }
    }

    #[test]
    fn test_title_in_svelte_head_reports_attribute_and_content_diagnostics() {
        let source = r#"<svelte:head>{#if visible}<title class="x">{name}<b>bad</b>{#if nested}bad{/if}</title>{/if}</svelte:head>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let illegal_attribute_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<title>` cannot have attributes nor directives")
            })
            .count();
        assert_eq!(illegal_attribute_errors, 1);

        let invalid_content_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`<title>` can only contain text and {tags}")
            })
            .count();
        assert_eq!(invalid_content_errors, 2);
    }

    #[test]
    fn test_title_diagnostics_only_apply_in_direct_svelte_head_context() {
        let source = r#"<title class="x"><b>ok</b></title><svelte:head><div><title class="x"><b>ok</b></title></div></svelte:head>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(
            !errors.iter().any(|error| {
                let message = error.to_string();
                message.contains("`<title>` cannot have attributes nor directives")
                    || message.contains("`<title>` can only contain text and {tags}")
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn test_special_tag_inside_textarea_reports_diagnostic() {
        let source = "<textarea>{@html value}</textarea><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("{@html ...} tag cannot be inside <textarea>")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_textarea_value_and_content_diagnostic() {
        let source = r#"<textarea value="x"> </textarea><textarea value></textarea><textarea bind:value={x}>text</textarea>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "A `<textarea>` can have either a value attribute or (equivalently) child content, but not both"
                ))
                .count(),
            1,
            "{errors:?}"
        );
    }

    #[test]
    fn test_raw_text_element_treats_tags_as_text_but_parses_mustaches() {
        let source = "<textarea><b>{value}</b></textarea>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "textarea");
                assert_eq!(el.children.len(), 3);
                assert!(matches!(&el.children[0], TemplateNode::Text(t) if t.data == "<b>"));
                assert!(
                    matches!(&el.children[1], TemplateNode::MustacheTag(tag) if tag.expression == "value")
                );
                assert!(matches!(&el.children[2], TemplateNode::Text(t) if t.data == "</b>"));
            }
            other => panic!("expected textarea element, got {other:?}"),
        }
    }

    #[test]
    fn test_element_close_tag_is_case_insensitive() {
        let source = "<DIV><SPAN>ok</span></DIV><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();

        assert_eq!(result.nodes.len(), 2);
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "DIV");
                assert_eq!(el.children.len(), 1);
                assert!(el.end_tag_span.is_some());
            }
            other => panic!("expected DIV element, got {other:?}"),
        }
    }

    #[test]
    fn test_raw_text_close_tag_is_case_insensitive() {
        let source = "<TEXTAREA><b>{value}</b></textarea><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();

        assert_eq!(result.nodes.len(), 2);
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "TEXTAREA");
                assert_eq!(el.children.len(), 3);
                assert!(matches!(&el.children[0], TemplateNode::Text(t) if t.data == "<b>"));
                assert!(
                    matches!(&el.children[1], TemplateNode::MustacheTag(tag) if tag.expression == "value")
                );
                assert!(matches!(&el.children[2], TemplateNode::Text(t) if t.data == "</b>"));
            }
            other => panic!("expected TEXTAREA element, got {other:?}"),
        }
    }

    #[test]
    fn test_implicit_close_is_case_insensitive() {
        let source = "<UL><LI>one<LI>two</UL>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();

        match &result.nodes[0] {
            TemplateNode::Element(ul) => {
                assert_eq!(ul.children.len(), 2);
                assert!(matches!(&ul.children[0], TemplateNode::Element(li) if li.name == "LI"));
                assert!(matches!(&ul.children[1], TemplateNode::Element(li) if li.name == "LI"));
            }
            other => panic!("expected UL element, got {other:?}"),
        }
    }

    #[test]
    fn test_p_does_not_implicitly_close_before_details_figure_or_figcaption() {
        let source = "<p><details>details</details><figure>figure</figure><figcaption>caption</figcaption></p>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();

        match &result.nodes[0] {
            TemplateNode::Element(paragraph) => {
                assert_eq!(paragraph.name, "p");
                let child_names: Vec<&str> = paragraph
                    .children
                    .iter()
                    .filter_map(|child| match child {
                        TemplateNode::Element(element) => Some(element.name.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(child_names, vec!["details", "figure", "figcaption"]);
            }
            other => panic!("expected p element, got {other:?}"),
        }
    }

    #[test]
    fn test_autoclosed_element_closing_tag_reports_svelte_diagnostic() {
        let source = "<p><div></div></p><span>after</span>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors.iter().any(|error| error.to_string().contains(
            "`</p>` attempted to close element that was already automatically closed by `<div>` (cannot nest `<div>` inside `<p>`)"
        )));
        assert_eq!(fragment.nodes.len(), 3);
        assert!(
            matches!(&fragment.nodes[2], TemplateNode::Element(element) if element.name == "span")
        );
    }

    #[test]
    fn test_tr_implicitly_closes_before_tbody_like_svelte() {
        let source = "<table><tr><td>one</td><tbody><tr><td>two</td></tr></tbody></table>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();

        match &result.nodes[0] {
            TemplateNode::Element(table) => {
                assert_eq!(table.name, "table");
                assert_eq!(table.children.len(), 2);
                assert!(matches!(&table.children[0], TemplateNode::Element(tr) if tr.name == "tr"));
                assert!(
                    matches!(&table.children[1], TemplateNode::Element(tbody) if tbody.name == "tbody")
                );
            }
            other => panic!("expected table element, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_comment() {
        let source = "<!-- a comment -->";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Comment(c) => assert_eq!(c.data, " a comment "),
            _ => panic!("expected Comment"),
        }
    }

    #[test]
    fn test_parse_snippet_block() {
        let source = "{#snippet greeting(name)}<p>Hello {name}</p>{/snippet}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::SnippetBlock(s) => {
                assert_eq!(s.name, "greeting");
                assert_eq!(s.params, "name");
            }
            _ => panic!("expected SnippetBlock"),
        }
    }

    #[test]
    fn test_snippet_rest_parameter_diagnostics() {
        let source =
            "{#snippet a(...args)}{/snippet}{#snippet b(first, ...rest)}{/snippet}{#snippet c({ ...rest })}{/snippet}{#snippet d([first, ...rest])}{/snippet}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("Snippets do not support rest parameters; use an array instead"))
                .count(),
            2,
            "{errors:?}"
        );
    }

    #[test]
    fn test_reserved_binding_identifier_diagnostics() {
        let source = r#"{#snippet if()}{/snippet}{#each items as if}{/each}{#each items as item, if}{/each}{#await promise then if}{/await}{#await promise catch if}{/await}{#await promise}{:then if}{:catch if}{/await}"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| {
                    error
                        .to_string()
                        .contains("'if' is a reserved word in JavaScript")
                })
                .count(),
            7,
            "{errors:?}"
        );
    }

    #[test]
    fn test_missing_binding_identifier_diagnostics() {
        let source = r#"{#snippet ()}{/snippet}{#each items as }{/each}{#each items as item, }{/each}{#await promise}{:then }{:catch }{/await}{#await promise then }{/await}"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(expected_identifier_message()))
                .count(),
            2,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(expected_pattern_message()))
                .count(),
            3,
            "{errors:?}"
        );
    }

    #[test]
    fn test_parse_generic_snippet_splits_header_parts() {
        let source = r#"{#snippet complex_generic<T extends { bracket: "<" } | "<" | Set<"<>">>(val: T)}{/snippet}"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::SnippetBlock(s) => {
                assert_eq!(s.name, "complex_generic");
                assert_eq!(
                    s.type_params.as_deref(),
                    Some(r#"T extends { bracket: "<" } | "<" | Set<"<>">"#)
                );
                assert_eq!(s.params, "val: T");
                assert_eq!(
                    &source[s.name_span.start as usize..s.name_span.end as usize],
                    "complex_generic"
                );
                let type_params_span = s.type_params_span.expect("type params span");
                assert_eq!(
                    &source[type_params_span.start as usize..type_params_span.end as usize],
                    r#"T extends { bracket: "<" } | "<" | Set<"<>">"#
                );
                let params_span = s.params_span.expect("params span");
                assert_eq!(
                    &source[params_span.start as usize..params_span.end as usize],
                    "val: T"
                );
            }
            _ => panic!("expected SnippetBlock"),
        }
    }

    #[test]
    fn test_parse_render_tag() {
        let source = "{@render greeting('world')}";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::RenderTag(r) => assert_eq!(r.expression, "greeting('world')"),
            _ => panic!("expected RenderTag"),
        }
    }

    #[test]
    fn test_special_tag_expression_diagnostics() {
        let source =
            "{@debug foo, bar.baz}{@debug foo, bar}{@render snippet}{@render snippet()}{@render snippet?.()}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_debug_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("{@debug ...} arguments must be identifiers")
            })
            .count();
        assert_eq!(invalid_debug_errors, 1, "{errors:?}");

        let invalid_render_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`{@render ...}` tags can only contain call expressions")
            })
            .count();
        assert_eq!(invalid_render_errors, 1, "{errors:?}");
    }

    #[test]
    fn test_render_tag_analyzer_diagnostics() {
        let source = "{@render snippet(...args)}{@render snippet.call(null)}{@render snippet.bind(null)}{@render snippet.apply(null)}{@render snippet?.(...args)}{@render snippet?.call(null)}{@render snippet()}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let spread_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("cannot use spread arguments in `{@render ...}` tags")
            })
            .count();
        assert_eq!(spread_errors, 2, "{errors:?}");

        let forbidden_call_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Calling a snippet function using apply, bind or call is not allowed")
            })
            .count();
        assert_eq!(forbidden_call_errors, 4, "{errors:?}");
    }

    #[test]
    fn test_const_tag_declaration_diagnostics() {
        let source =
            "{#if ok}{@const a}{@const a = b, c}{@const a = (b, c)}{@const { value } = item}{/if}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let missing_equals = errors
            .iter()
            .filter(|error| error.to_string().contains("Expected token ="))
            .count();
        assert_eq!(missing_equals, 1, "{errors:?}");

        let invalid_declarations = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("{@const ...} must consist of a single variable declaration")
            })
            .count();
        assert_eq!(invalid_declarations, 1, "{errors:?}");
    }

    #[test]
    fn test_const_tag_placement_diagnostics() {
        let source =
            "{@const root = 1}<div>{@const nested = 1}</div>{#if ok}<svelte:self>{@const self = 1}</svelte:self>{/if}";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let placement_errors = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains(const_tag_invalid_placement_message())
            })
            .count();
        assert_eq!(placement_errors, 3, "{errors:?}");
    }

    #[test]
    fn test_const_tag_valid_placements() {
        let source = r#"{#if ok}{@const a = 1}{:else}{@const b = 2}{/if}{#each items as item}{@const c = item}{:else}{@const d = 0}{/each}{#await promise}{@const e = 1}{:then value}{@const f = value}{:catch err}{@const g = err}{/await}{#key value}{@const h = value}{/key}{#snippet row()}{@const i = 1}{/snippet}<Component>{@const j = 1}</Component><Component><div slot="x">{@const k = 1}</div></Component><Component><svelte:fragment slot="y">{@const l = 1}</svelte:fragment></Component><svelte:boundary>{@const m = 1}</svelte:boundary>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(
            !errors.iter().any(|error| error
                .to_string()
                .contains(const_tag_invalid_placement_message())),
            "{errors:?}"
        );
    }

    #[test]
    fn test_text_node_invalid_placement_diagnostics() {
        let source = "<table>{value}text{#if ok}{inside}{/if}<tbody><tr>{cell}</tr></tbody><tbody>row</tbody><slot>fallback</slot>{#snippet child()}safe{/snippet}</table><table> \n\t </table>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let placement_errors = errors
            .iter()
            .filter(|error| error.to_string().contains("`<#text>` cannot be a child"))
            .count();
        assert_eq!(placement_errors, 6, "{errors:?}");

        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("`<table>` only allows these children")));
        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("`<tr>` only allows these children")));
        assert!(errors.iter().any(|error| error
            .to_string()
            .contains("`<tbody>` only allows these children")));
    }

    #[test]
    fn test_element_invalid_placement_diagnostics() {
        let source = "<table><div></div></table><div><tr></tr></div><p><span><div></div></span></p><a><span><a></a></span></a><table>{#if ok}<div></div><tbody><div></div></tbody><tbody>{#if nested}<div></div>{/if}</tbody>{/if}</table>";
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let placement_errors = errors
            .iter()
            .filter(|error| {
                error.to_string().contains("node_invalid_placement")
                    || error.to_string().contains("The browser will 'repair'")
            })
            .count();
        assert_eq!(placement_errors, 5, "{errors:?}");

        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("`<div>` cannot be a child of `<table>`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("`<tr>` must be the child of a `<thead>`, `<tbody>`, or `<tfoot>`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("`<div>` cannot be a descendant of `<p>`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("`<a>` cannot be a descendant of `<a>`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .to_string()
                .contains("`<div>` cannot be a child of `<tbody>`")
        }));
    }

    #[test]
    fn test_attribute_analyzer_diagnostics() {
        let source = r#"<button on:click|foo={bar}></button><button on:click|passive|preventDefault={bar}></button><button onclick="foo()"></button><div style:color|foo={bar}></div><button on:click|once={bar}></button><button onclick={foo}></button><div style:color|important={bar}></div><Component on:click|capture={bar}/><Component on:click|foo={bar}/><Component on:click|once={bar}/><Component use:foo/><Component class:active/><Component style:color/><Component transition:fade/><Component animate:flip/>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_event_modifiers = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains(event_handler_invalid_modifier_message())
            })
            .count();
        assert_eq!(invalid_event_modifiers, 1, "{errors:?}");

        let modifier_combinations = errors
            .iter()
            .filter(|error| {
                error.to_string().contains(
                    "The 'passive' and 'preventDefault' modifiers cannot be used together",
                )
            })
            .count();
        assert_eq!(modifier_combinations, 1, "{errors:?}");

        let invalid_event_attributes = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Event attribute must be a JavaScript expression, not a string")
            })
            .count();
        assert_eq!(invalid_event_attributes, 1, "{errors:?}");

        let invalid_style_modifiers = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("`style:` directive can only use the `important` modifier")
            })
            .count();
        assert_eq!(invalid_style_modifiers, 1, "{errors:?}");

        let invalid_component_modifiers = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Event modifiers other than 'once' can only be used on DOM elements")
            })
            .count();
        assert_eq!(invalid_component_modifiers, 2, "{errors:?}");

        let invalid_component_directives = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains(component_invalid_directive_message())
            })
            .count();
        assert_eq!(invalid_component_directives, 5, "{errors:?}");
    }

    #[test]
    fn test_mixed_event_handler_syntax_diagnostics() {
        let alloc = oxc::allocator::Allocator::default();

        let source = r#"<button on:click={old}></button>{#if ok}<div onclick={modern}></div>{/if}"#;
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("Mixing old (on:click) and new syntaxes"))
                .count(),
            1,
            "{errors:?}"
        );

        for source in [
            r#"<Component on:click={old} onclick={modern} />"#,
            r#"<svelte:window on:click={old} onclick={modern} />"#,
            r#"<button onfoo="modern"></button><button on:click={old}></button>"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert!(
                !errors
                    .iter()
                    .any(|error| error.to_string().contains("Mixing old (on:")),
                "{source}: {errors:?}"
            );
        }
    }

    #[test]
    fn test_invalid_arguments_usage_diagnostics() {
        let alloc = oxc::allocator::Allocator::default();

        for source in [
            r#"{arguments}"#,
            r#"{() => arguments}"#,
            r#"<div data-x={arguments}></div>"#,
            r#"{#if arguments}x{/if}"#,
            r#"{@render arguments()}"#,
            r#"{@debug arguments}"#,
            r#"{#if ok}{@const x = arguments}{/if}"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert!(
                errors.iter().any(|error| error
                    .to_string()
                    .contains(invalid_arguments_usage_message())),
                "{source}: {errors:?}"
            );
        }

        for source in [
            r#"{function f() { return arguments }}"#,
            r#"<div data-x={function f() { return arguments }}></div>"#,
            r#"{#if ok}{@const x = function f() { return arguments }}{/if}"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert!(
                !errors.iter().any(|error| error
                    .to_string()
                    .contains(invalid_arguments_usage_message())),
                "{source}: {errors:?}"
            );
        }
    }

    #[test]
    fn test_invalid_await_usage_diagnostics() {
        let alloc = oxc::allocator::Allocator::default();

        for source in [
            r#"{await foo}"#,
            r#"<div data-x={await foo}></div>"#,
            r#"<button on:click={await foo}></button>"#,
            r#"{#if await foo}x{/if}"#,
            r#"{#each await items as item}{/each}"#,
            r#"{#key await key}{/key}"#,
            r#"{#await await promise}{/await}"#,
            r#"{@render snippet(await foo)}"#,
            r#"{#if ok}{@const x = await foo}{/if}"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(experimental_async_message())),
                "{source}: {errors:?}"
            );
        }

        for source in [
            r#"{async () => await foo}"#,
            r#"<div data-x={async () => await foo}></div>"#,
            r#"{#if async () => await foo}x{/if}"#,
            r#"{@render snippet(async () => await foo)}"#,
            r#"{#if ok}{@const x = async () => await foo}{/if}"#,
        ] {
            let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
            assert!(
                !errors
                    .iter()
                    .any(|error| error.to_string().contains(experimental_async_message())),
                "{source}: {errors:?}"
            );
        }
    }

    #[test]
    fn test_attribute_invalid_name_diagnostics() {
        let source = r#"<div 1foo .foo -foo foo|bar foo.bar></div><svelte:element this="div" 2foo></svelte:element><Component 1foo/>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_names = errors
            .iter()
            .filter(|error| error.to_string().contains("is not a valid attribute name"))
            .count();
        assert_eq!(invalid_names, 5, "{errors:?}");

        assert!(
            !errors.iter().any(|error| error
                .to_string()
                .contains("'foo.bar' is not a valid attribute name")),
            "{errors:?}"
        );
    }

    #[test]
    fn test_motion_directive_diagnostics() {
        let source = r#"<div animate:flip></div>{#each items as item}<div animate:flip></div>{/each}{#each items as item (item.id)}<div animate:a animate:b></div>{/each}{#each items as item (item.id)}<div animate:flip></div><span></span>{/each}{#each items as item (item.id)}{@const x = item}<div animate:flip></div>{/each}{#each items as item (item.id)}{#if ok}<div animate:flip></div>{/if}{/each}<div transition:fade transition:fly></div><div in:fade transition:fly></div><div in:fade out:fly></div><div in:fade in:fly></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        let invalid_animation_placement = errors
            .iter()
            .filter(|error| {
                let message = error.to_string();
                message.contains(animation_invalid_placement_message())
                    && !message.contains("Did you forget to add a key")
            })
            .count();
        assert_eq!(invalid_animation_placement, 3, "{errors:?}");

        let missing_animation_key = errors
            .iter()
            .filter(|error| error.to_string().contains(animation_missing_key_message()))
            .count();
        assert_eq!(missing_animation_key, 1, "{errors:?}");

        let duplicate_animation = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("An element can only have one 'animate' directive")
            })
            .count();
        assert_eq!(duplicate_animation, 1, "{errors:?}");

        let duplicate_transitions = errors
            .iter()
            .filter(|error| {
                let message = error.to_string();
                message.contains("Cannot use multiple `transition:` directives")
                    || message.contains("Cannot use multiple `in:` directives")
            })
            .count();
        assert_eq!(duplicate_transitions, 2, "{errors:?}");

        let transition_conflicts = errors
            .iter()
            .filter(|error| {
                error
                    .to_string()
                    .contains("Cannot use `in:` alongside existing `transition:` directive")
            })
            .count();
        assert_eq!(transition_conflicts, 1, "{errors:?}");
    }

    #[test]
    fn test_bind_directive_analyzer_diagnostics() {
        let source = r#"<div bind:foo={x}></div><div bind:currentTime={x}></div><input type="text" bind:checked={x}><input type="text" bind:files={x}><input type={kind} bind:checked={x}><select multiple={x} bind:value={y}></select><div bind:innerText={x}></div><div contenteditable={editable} bind:innerHTML={x}></div><audio bind:currentTime={x}></audio><input type="checkbox" bind:checked={x}><input type="file" bind:files={x}><select multiple bind:value={y}></select><div contenteditable bind:textContent={x}></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("`bind:foo` is not a valid binding"))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("`bind:currentTime` can only be used with `<audio>`, `<video>`"))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("`bind:checked` can only be used with `<input type=\"checkbox\">`"))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("`bind:files` can only be used with `<input type=\"file\">`"))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "'type' attribute must be a static text value if input uses two-way binding"
                ))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("'multiple' attribute must be static if select uses two-way binding"))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "'contenteditable' attribute is required for textContent, innerHTML and innerText two-way bindings"
                ))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "'contenteditable' attribute cannot be dynamic if element uses two-way binding"
                ))
                .count(),
            1,
            "{errors:?}"
        );
    }

    #[test]
    fn test_bind_target_special_element_diagnostics() {
        let source = r#"<svelte:window bind:value={x}/><svelte:window bind:clientWidth={x}/><svelte:document bind:clientWidth={x}/><svg bind:offsetWidth={x}></svg><div bind:offsetWidth={x}></div><svelte:window bind:innerWidth={x}/><svelte:document bind:activeElement={x}/>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "`bind:value` can only be used with `<input>`, `<textarea>`, `<select>`"
                ))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "`bind:clientWidth` is not a valid binding. Possible bindings for <svelte:window>"
                ))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error.to_string().contains(
                    "`bind:clientWidth` is not a valid binding. Possible bindings for <svelte:document>"
                ))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("`bind:offsetWidth` can only be used with non-`<svg>` elements"))
                .count(),
            1,
            "{errors:?}"
        );
    }

    #[test]
    fn test_bind_invalid_name_suggestions() {
        let source = r#"<input bind:cheked={x}><div bind:clientWidht={x}></div><svelte:window bind:innerWidht={x}/><div bind:cheked={x}></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        for message in [
            "`bind:cheked` is not a valid binding. Did you mean 'checked'?",
            "`bind:clientWidht` is not a valid binding. Did you mean 'clientWidth'?",
            "`bind:innerWidht` is not a valid binding. Did you mean 'innerWidth'?",
        ] {
            assert!(
                errors
                    .iter()
                    .any(|error| error.to_string().contains(message)),
                "{message}: {errors:?}"
            );
        }

        assert!(errors.iter().any(|error| {
            let message = error.to_string();
            message.contains("`bind:cheked` is not a valid binding")
                && !message.contains("Did you mean 'checked'?")
        }));
    }

    #[test]
    fn test_runes_attribute_value_diagnostics() {
        let source = r#"<svelte:options runes={true}/><div class={bar}foo></div><div class=foo{bar}></div><div foo={a,b}></div><div foo={(a,b)}></div><div foo="{a,b}"></div><Component prop={a,b}/><Component prop={(a,b)}/><Component prop={bar}baz/>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains(attribute_unquoted_sequence_message()))
                .count(),
            3,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains(attribute_invalid_sequence_expression_message()))
                .count(),
            3,
            "{errors:?}"
        );

        let source = r#"<svelte:options runes={false}/><div class={bar}foo foo={a,b}></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);
        assert!(
            !errors.iter().any(|error| {
                let message = error.to_string();
                message.contains(attribute_unquoted_sequence_message())
                    || message.contains(attribute_invalid_sequence_expression_message())
            }),
            "{errors:?}"
        );
    }

    #[test]
    fn test_bind_expression_and_illegal_await_diagnostics() {
        let source = r#"<input bind:value={foo()}><input bind:value={(get, set)}><input bind:value={get, set, extra}><input type="checkbox" bind:group={get, set}><input bind:value={foo[await bar]}><input bind:value={async () => await get(), value => set(value)}><div use:action={await setup}></div><div transition:fade={await opts}></div>{#each items as item (item)}<div animate:flip={await opts}></div>{/each}<div {@attach await attach}></div><div use:action={async () => await ok()}></div><input bind:value={foo[async () => await ok()]}>"#;
        let alloc = oxc::allocator::Allocator::default();
        let (_fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains(bind_invalid_expression_message()))
                .count(),
            2,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains("`bind:value={get, set}` must not have surrounding parentheses"))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains(bind_group_invalid_expression_message()))
                .count(),
            1,
            "{errors:?}"
        );
        assert_eq!(
            errors
                .iter()
                .filter(|error| error
                    .to_string()
                    .contains(illegal_await_expression_message()))
                .count(),
            6,
            "{errors:?}"
        );
    }

    #[test]
    fn test_parse_directive() {
        let source = r#"<button on:click|preventDefault={handler}>Click</button>"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.attributes.len(), 1);
                match &el.attributes[0] {
                    Attribute::Directive {
                        kind,
                        name,
                        modifiers,
                        ..
                    } => {
                        assert!(matches!(kind, DirectiveKind::EventHandler));
                        assert_eq!(name, "click");
                        assert_eq!(modifiers, &["preventDefault"]);
                    }
                    _ => panic!("expected Directive"),
                }
            }
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn test_unquoted_attribute_before_self_closing_component() {
        let source = "<Component foo=bar/>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.name, "Component");
                assert!(el.self_closing);
                match &el.attributes[0] {
                    Attribute::NormalAttribute { name, value, .. } => {
                        assert_eq!(name, "foo");
                        assert!(matches!(value, AttributeValue::Static(v) if v == "bar"));
                    }
                    _ => panic!("expected normal attribute"),
                }
            }
            _ => panic!("expected Element"),
        }
    }

    #[test]
    fn test_unquoted_attribute_expression_then_text_is_one_sequence() {
        let source = "<div class={bar}foo></div>";
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => {
                assert_eq!(el.attributes.len(), 1);
                match &el.attributes[0] {
                    Attribute::NormalAttribute { name, value, span } => {
                        assert_eq!(name, "class");
                        assert_eq!(span.end, 19);
                        match value {
                            AttributeValue::Concat(parts) => {
                                assert!(
                                    matches!(&parts[0], AttributeValuePart::Expression(v) if v == "bar")
                                );
                                assert!(
                                    matches!(&parts[1], AttributeValuePart::Static(v) if v == "foo")
                                );
                            }
                            other => panic!("expected concat value, got {other:?}"),
                        }
                    }
                    other => panic!("expected normal attribute, got {other:?}"),
                }
            }
            other => panic!("expected Element, got {other:?}"),
        }
    }

    #[test]
    fn test_missing_attribute_value_reports_diagnostic_and_recovers() {
        let source = "<div foo=></div><p>after</p>";
        let alloc = oxc::allocator::Allocator::default();
        let (fragment, errors) = parse_fragment_with_errors(source, &alloc);

        assert!(errors
            .iter()
            .any(|error| error.to_string().contains("Expected attribute value")));
        assert_eq!(fragment.nodes.len(), 2);
        match &fragment.nodes[1] {
            TemplateNode::Element(el) => assert_eq!(el.name, "p"),
            other => panic!("expected trailing paragraph, got {other:?}"),
        }
    }

    #[test]
    fn test_concat_attribute_expression_with_string_brace() {
        let source = r#"<div title="a {format('}')} b"></div>"#;
        let alloc = oxc::allocator::Allocator::default();
        let result = parse_fragment(source, &alloc).unwrap();
        match &result.nodes[0] {
            TemplateNode::Element(el) => match &el.attributes[0] {
                Attribute::NormalAttribute { value, .. } => match value {
                    AttributeValue::Concat(parts) => {
                        assert!(matches!(&parts[0], AttributeValuePart::Static(v) if v == "a "));
                        assert!(
                            matches!(&parts[1], AttributeValuePart::Expression(v) if v == "format('}')")
                        );
                        assert!(matches!(&parts[2], AttributeValuePart::Static(v) if v == " b"));
                    }
                    _ => panic!("expected concat value"),
                },
                _ => panic!("expected normal attribute"),
            },
            _ => panic!("expected Element"),
        }
    }
}
