//! Svelte template parser.
//!
//! Parses the template portion of a `.svelte` file (everything outside `<script>` and
//! `<style>` blocks) into a tree of [`TemplateNode`]s.

use oxc_diagnostics::OxcDiagnostic;
use oxc::span::Span;
use crate::ast::*;

/// Parse a template source string into a [`Fragment`].
///
/// Parse a template source string into a [`Fragment`].
pub fn parse_fragment(source: &str) -> Result<Fragment, OxcDiagnostic> {
    let mut parser = TemplateParser::new(source);
    parser.parse_fragment()
}

/// The template parser state machine.
struct TemplateParser<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> TemplateParser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, pos: 0 }
    }

    /// Parse the entire template into a fragment.
    fn parse_fragment(&mut self) -> Result<Fragment, OxcDiagnostic> {
        self.parse_fragment_with_parent(None)
    }

    /// Parse a fragment, optionally with a parent element name for implicit closing.
    fn parse_fragment_with_parent(&mut self, parent: Option<&str>) -> Result<Fragment, OxcDiagnostic> {
        let start = self.pos as u32;
        let mut nodes = Vec::new();

        while self.pos < self.source.len() {
            // Skip over <script> and <style> blocks at top level only
            if parent.is_none() && (self.looking_at("<script") || self.looking_at("<style")) {
                self.skip_block()?;
                continue;
            }

            // Check for implicit closing (e.g., <li> closes previous <li>)
            if let Some(parent_name) = parent {
                if self.looking_at("<") && !self.looking_at("</") && !self.looking_at("<!") {
                    let peek_name = self.peek_tag_name();
                    if should_implicitly_close(parent_name, &peek_name) {
                        break;
                    }
                }
            }

            if self.looking_at("</") {
                // Check if the closing tag matches the parent
                if let Some(parent_name) = parent {
                    let close_name = self.peek_close_tag_name();
                    if !close_name.is_empty() && close_name != parent_name {
                        // Closing tag doesn't match parent — parent is auto-closed
                        break;
                    }
                }
                // Closing tag — let the caller handle it
                break;
            } else if self.looking_at("{/") {
                // Block closing tag — let the caller handle it
                break;
            } else if self.looking_at("{:") {
                // Block continuation tag — let the caller handle it
                break;
            } else if self.looking_at("<!--") {
                nodes.push(self.parse_comment()?);
            } else if self.looking_at("{#if") {
                nodes.push(self.parse_if_block()?);
            } else if self.looking_at("{#each") {
                nodes.push(self.parse_each_block()?);
            } else if self.looking_at("{#await") {
                nodes.push(self.parse_await_block()?);
            } else if self.looking_at("{#key") {
                nodes.push(self.parse_key_block()?);
            } else if self.looking_at("{#snippet") {
                nodes.push(self.parse_snippet_block()?);
            } else if self.looking_at("{@html") {
                nodes.push(self.parse_raw_mustache()?);
            } else if self.looking_at("{@debug") {
                nodes.push(self.parse_debug_tag()?);
            } else if self.looking_at("{@const") {
                nodes.push(self.parse_const_tag()?);
            } else if self.looking_at("{@render") {
                nodes.push(self.parse_render_tag()?);
            } else if self.looking_at("{") {
                match self.parse_mustache() {
                    Ok(node) => nodes.push(node),
                    Err(_) => {
                        // Error recovery for malformed mustache
                        let start = self.pos as u32;
                        self.pos += 1; // skip {
                        let data = "{".to_string();
                        nodes.push(TemplateNode::Text(Text {
                            data,
                            span: Span::new(start, self.pos as u32),
                        }));
                    }
                }
            } else if self.looking_at("<") {
                match self.parse_element() {
                    Ok(node) => nodes.push(node),
                    Err(_) => {
                        // Error recovery: skip to next > or newline
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
                        // Emit the skipped content as a text node
                        if self.pos as u32 > recovery_start {
                            let data = self.source[recovery_start as usize..self.pos].to_string();
                            nodes.push(TemplateNode::Text(Text {
                                data,
                                span: Span::new(recovery_start, self.pos as u32),
                            }));
                        }
                    }
                }
            } else {
                nodes.push(self.parse_text()?);
            }
        }

        Ok(Fragment {
            nodes,
            span: Span::new(start, self.pos as u32),
        })
    }

    // ─── Helpers ───────────────────────────────────────────────────────

    fn looking_at(&self, prefix: &str) -> bool {
        self.source[self.pos..].starts_with(prefix)
    }

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

    fn skip_whitespace(&mut self) {
        while self.pos < self.source.len()
            && self.source.as_bytes()[self.pos].is_ascii_whitespace()
        {
            self.pos += 1;
        }
    }

    /// Parse children of raw text elements (textarea, title).
    /// HTML tags are treated as text, but mustache expressions are parsed.
    fn parse_raw_text_children(&mut self, tag_name: &str) -> Result<Vec<TemplateNode>, OxcDiagnostic> {
        let close_prefix = format!("</{}", tag_name);
        let mut nodes = Vec::new();

        while self.pos < self.source.len() {
            // Check for closing tag — </tagname followed by whitespace or >
            if self.looking_at(&close_prefix) {
                let after_prefix = &self.source[self.pos + close_prefix.len()..];
                let next_ch = after_prefix.chars().next();
                if next_ch == Some('>') || next_ch.map(|c| c.is_ascii_whitespace()).unwrap_or(true) {
                    // Valid closing tag — eat to >
                    self.eat_until(">");
                    if self.looking_at(">") {
                        self.eat(">")?;
                    }
                    break;
                }
                // Not a valid closing tag (e.g., </textaread) — treat as text
            }

            if self.looking_at("{") && !self.looking_at("{{") {
                // Mustache expression
                nodes.push(self.parse_mustache()?);
            } else {
                // Raw text until next { or closing tag prefix
                let text_start = self.pos as u32;
                while self.pos < self.source.len() && !self.looking_at("{") {
                    if self.looking_at(&close_prefix) {
                        let after_prefix = &self.source[self.pos + close_prefix.len()..];
                        let next_ch = after_prefix.chars().next();
                        if next_ch == Some('>') || next_ch.map(|c| c.is_ascii_whitespace()).unwrap_or(true) {
                            break;
                        }
                    }
                    self.pos += 1;
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

        Ok(nodes)
    }

    /// Peek at the closing tag name (e.g., "</div>" → "div") without advancing.
    fn peek_close_tag_name(&self) -> String {
        let remaining = self.remaining();
        if !remaining.starts_with("</") { return String::new(); }
        let after = &remaining[2..];
        let end = after.find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != ':' && c != '.')
            .unwrap_or(after.len());
        after[..end].to_string()
    }

    /// Peek at the next tag name without advancing the parser position.
    fn peek_tag_name(&self) -> String {
        let remaining = self.remaining();
        if !remaining.starts_with('<') { return String::new(); }
        let after_lt = &remaining[1..];
        let end = after_lt.find(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_' && c != ':').unwrap_or(after_lt.len());
        after_lt[..end].to_string()
    }

    /// Skip a `<script>` or `<style>` block entirely.
    fn skip_block(&mut self) -> Result<(), OxcDiagnostic> {
        let is_script = self.looking_at("<script");
        let close_prefix = if is_script { "</script" } else { "</style" };
        let close_tag_exact = if is_script { "</script>" } else { "</style>" };

        // Try exact match first, then prefix with whitespace
        loop {
            self.eat_until(close_prefix);
            if self.pos >= self.source.len() {
                break;
            }
            if self.looking_at(close_tag_exact) {
                self.pos += close_tag_exact.len();
                break;
            }
            if self.looking_at(close_prefix) {
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
                b'\'' | b'"' | b'`' => self.skip_string_literal(bytes[self.pos])?,
                _ => {}
            }
            self.pos += 1;
        }

        Ok(self.source[start..self.pos].to_string())
    }

    /// Skip a string literal (handles escaped quotes).
    fn skip_string_literal(&mut self, quote: u8) -> Result<(), OxcDiagnostic> {
        self.pos += 1; // skip opening quote
        let bytes = self.source.as_bytes();
        while self.pos < self.source.len() {
            if bytes[self.pos] == b'\\' {
                self.pos += 1; // skip escaped char
            } else if bytes[self.pos] == quote {
                return Ok(());
            }
            self.pos += 1;
        }
        Err(OxcDiagnostic::error("Unterminated string literal"))
    }

    // ─── Node parsers ──────────────────────────────────────────────────

    fn parse_text(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        let data = self.eat_until_any(&["<", "{", "<!--"]);
        Ok(TemplateNode::Text(Text {
            data: data.to_string(),
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_comment(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("<!--")?;
        let data = self.eat_until("-->");
        self.eat("-->")?;
        Ok(TemplateNode::Comment(Comment {
            data: data.to_string(),
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_mustache(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{")?;
        let expression = self.read_expression()?;
        self.eat("}")?;
        Ok(TemplateNode::MustacheTag(MustacheTag {
            expression,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_raw_mustache(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@html")?;
        self.skip_whitespace();
        let expression = self.read_expression()?;
        self.eat("}")?;
        Ok(TemplateNode::RawMustacheTag(RawMustacheTag {
            expression,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_debug_tag(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@debug")?;
        self.skip_whitespace();
        let idents_str = self.eat_until("}");
        let identifiers = idents_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self.eat("}")?;
        Ok(TemplateNode::DebugTag(DebugTag {
            identifiers,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_const_tag(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@const")?;
        self.skip_whitespace();
        let declaration = self.read_expression()?;
        self.eat("}")?;
        Ok(TemplateNode::ConstTag(ConstTag {
            declaration,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_render_tag(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{@render")?;
        self.skip_whitespace();
        let expression = self.read_expression()?;
        self.eat("}")?;
        Ok(TemplateNode::RenderTag(RenderTag {
            expression,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_element(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("<")?;

        // Parse tag name (allow ! for <!doctype>)
        let name_start = self.pos;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' || ch == b':' || ch == b'.'
                || (ch == b'!' && self.pos == name_start) {
                self.pos += 1;
            } else {
                break;
            }
        }
        let name = self.source[name_start..self.pos].to_string();

        // Parse attributes
        let attributes = self.parse_attributes()?;

        // Check for self-closing or void element
        self.skip_whitespace();
        let self_closing = if self.looking_at("/>") {
            self.eat("/>")?;
            true
        } else if self.looking_at(">") {
            self.eat(">")?;
            false
        } else {
            // No > found — unclosed opening tag. Treat as self-closing.
            // This handles cases like `<Comp foo={bar}` without closing >
            true
        };

        let is_void = is_void_element(&name);

        let is_raw_text = name == "textarea" || name == "title";
        let children = if self_closing || is_void {
            Vec::new()
        } else if is_raw_text {
            // Raw text elements: parse as text with mustache expressions, no HTML elements
            self.parse_raw_text_children(&name)?
        } else {
            // Parse children until closing tag (with implicit closing for li, p, etc.)
            let fragment = self.parse_fragment_with_parent(Some(&name))?;
            // Eat closing tag if present
            if self.looking_at(&format!("</{}", name)) {
                self.eat_until(">");
                self.eat(">")?;
            }
            fragment.nodes
        };

        Ok(TemplateNode::Element(Element {
            name,
            attributes,
            children,
            self_closing,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_attributes(&mut self) -> Result<Vec<Attribute>, OxcDiagnostic> {
        let mut attributes = Vec::new();

        loop {
            self.skip_whitespace();

            if self.pos >= self.source.len()
                || self.looking_at(">")
                || self.looking_at("/>")
                || self.looking_at("</")
                || self.looking_at("<")
                || self.looking_at("{#")
                || self.looking_at("{:")
                || self.looking_at("{/")
                || self.looking_at("{@")
            {
                break;
            }

            // Spread attribute: {...expr}
            if self.looking_at("{...") {
                let start = self.pos as u32;
                self.eat("{")?;
                self.read_expression()?;
                self.eat("}")?;
                attributes.push(Attribute::Spread {
                    span: Span::new(start, self.pos as u32),
                });
                continue;
            }

            // Shorthand attribute: {name}
            if self.looking_at("{") {
                let start = self.pos as u32;
                self.eat("{")?;
                let expr = self.read_expression()?;
                self.eat("}")?;
                attributes.push(Attribute::NormalAttribute {
                    name: expr.clone(),
                    value: AttributeValue::Expression(expr),
                    span: Span::new(start, self.pos as u32),
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

            if attr_name.is_empty() {
                // Unexpected character
                self.pos += 1;
                continue;
            }

            // Check if this is a directive
            if let Some(directive) = parse_directive_name(attr_name) {
                // Check for value
                let name_end = self.pos;
                self.skip_whitespace();
                if self.looking_at("=") {
                    self.eat("=")?;
                    self.skip_whitespace();
                    self.parse_attribute_value()?;
                    attributes.push(Attribute::Directive {
                        kind: directive.0,
                        name: directive.1.to_string(),
                        modifiers: directive.2.iter().map(|s| s.to_string()).collect(),
                        span: Span::new(attr_start, self.pos as u32),
                    });
                } else {
                    // No value — span ends at the directive name end
                    attributes.push(Attribute::Directive {
                        kind: directive.0,
                        name: directive.1.to_string(),
                        modifiers: directive.2.iter().map(|s| s.to_string()).collect(),
                        span: Span::new(attr_start, name_end as u32),
                    });
                }
                continue;
            }

            // Regular attribute — check for value
            self.skip_whitespace();
            let value = if self.looking_at("=") {
                self.eat("=")?;
                self.skip_whitespace();
                self.parse_attribute_value()?
            } else {
                AttributeValue::True
            };

            attributes.push(Attribute::NormalAttribute {
                name: attr_name.to_string(),
                value,
                span: Span::new(attr_start, self.pos as u32),
            });
        }

        Ok(attributes)
    }

    fn parse_attribute_value(&mut self) -> Result<AttributeValue, OxcDiagnostic> {
        if self.looking_at("{") {
            self.eat("{")?;
            let expr = self.read_expression()?;
            self.eat("}")?;
            Ok(AttributeValue::Expression(expr))
        } else if self.looking_at("\"") {
            self.eat("\"")?;
            let value = self.eat_until("\"");
            self.eat("\"")?;
            // Check for embedded expressions
            if value.contains('{') {
                Ok(parse_concat_value(value))
            } else {
                Ok(AttributeValue::Static(value.to_string()))
            }
        } else if self.looking_at("'") {
            self.eat("'")?;
            let value = self.eat_until("'");
            self.eat("'")?;
            if value.contains('{') {
                Ok(parse_concat_value(value))
            } else {
                Ok(AttributeValue::Static(value.to_string()))
            }
        } else {
            // Unquoted value (read until whitespace or >)
            // Note: / is allowed in unquoted values (e.g., href=/)
            let start = self.pos;
            while self.pos < self.source.len() {
                let ch = self.source.as_bytes()[self.pos];
                if ch.is_ascii_whitespace() || ch == b'>' {
                    break;
                }
                self.pos += 1;
            }
            let value = &self.source[start..self.pos];
            if value.contains('{') {
                Ok(parse_concat_value(value))
            } else {
                Ok(AttributeValue::Static(value.to_string()))
            }
        }
    }

    // ─── Block parsers ─────────────────────────────────────────────────

    fn parse_if_block(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#if")?;
        self.skip_whitespace();
        let test = self.read_expression()?;
        self.eat("}")?;

        let consequent = self.parse_fragment()?;

        let alternate = if self.looking_at("{:else if") {
            Some(Box::new(self.parse_else_if_block()?))
        } else if self.looking_at("{:else}") {
            let else_start = self.pos as u32;
            self.eat("{:else}")?;
            let content_start = self.pos as u32;
            let alt = self.parse_fragment()?;
            let else_end = self.pos as u32;
            // Wrap in a synthetic IfBlock with empty test to represent :else
            // Use content_start..else_end as the span (after {:else} to before {/if})
            Some(Box::new(TemplateNode::IfBlock(IfBlock {
                test: String::new(),
                consequent: alt,
                alternate: None,
                span: Span::new(content_start, else_end),
            })))
        } else {
            None
        };

        if self.looking_at("{/if}") {
            self.eat("{/if}")?;
        }

        Ok(TemplateNode::IfBlock(IfBlock {
            test,
            consequent,
            alternate,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_else_if_block(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{:else if")?;
        self.skip_whitespace();
        let test = self.read_expression()?;
        self.eat("}")?;

        let consequent = self.parse_fragment()?;

        let alternate = if self.looking_at("{:else if") {
            Some(Box::new(self.parse_else_if_block()?))
        } else if self.looking_at("{:else}") {
            let else_start = self.pos as u32;
            self.eat("{:else}")?;
            let content_start = self.pos as u32;
            let alt = self.parse_fragment()?;
            let else_end = self.pos as u32;
            Some(Box::new(TemplateNode::IfBlock(IfBlock {
                test: String::new(),
                consequent: alt,
                alternate: None,
                span: Span::new(content_start, else_end),
            })))
        } else {
            None
        };

        Ok(TemplateNode::IfBlock(IfBlock {
            test,
            consequent,
            alternate,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_each_block(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#each")?;
        self.skip_whitespace();

        // Parse: expression as context, index (key)
        let header = self.eat_until("}");
        self.eat("}")?;

        let (expression, context, index, key) = parse_each_header(header);

        let body = self.parse_fragment()?;

        let fallback = if self.looking_at("{:else}") {
            self.eat("{:else}")?;
            Some(self.parse_fragment()?)
        } else {
            None
        };

        let closed = self.looking_at("{/each}");
        if closed {
            self.eat("{/each}")?;
        }

        let mut end = self.pos as u32;
        // For unclosed blocks, trim trailing whitespace from span
        if !closed {
            while end > start && self.source.as_bytes()[(end - 1) as usize].is_ascii_whitespace() {
                end -= 1;
            }
        }

        Ok(TemplateNode::EachBlock(EachBlock {
            expression,
            context,
            index,
            key,
            body,
            fallback,
            span: Span::new(start, end),
        }))
    }

    fn parse_await_block(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#await")?;
        self.skip_whitespace();
        let expression = self.eat_until("}").trim().to_string();
        self.eat("}")?;

        let pending = Some(self.parse_fragment()?);
        let mut then = None;
        let mut then_binding = None;
        let mut catch = None;
        let mut catch_binding = None;

        if self.looking_at("{:then") {
            let then_tag_start = self.pos as u32;
            self.eat("{:then")?;
            self.skip_whitespace();
            let binding = self.eat_until("}").trim().to_string();
            if !binding.is_empty() {
                then_binding = Some(binding);
            }
            self.eat("}")?;
            let mut frag = self.parse_fragment()?;
            // Set the fragment span to start at the {:then} tag
            frag.span = Span::new(then_tag_start, frag.span.end);
            then = Some(frag);
        }

        if self.looking_at("{:catch") {
            let catch_tag_start = self.pos as u32;
            self.eat("{:catch")?;
            self.skip_whitespace();
            let binding = self.eat_until("}").trim().to_string();
            if !binding.is_empty() {
                catch_binding = Some(binding);
            }
            self.eat("}")?;
            let mut frag = self.parse_fragment()?;
            // Set the fragment span to start at the {:catch} tag
            frag.span = Span::new(catch_tag_start, frag.span.end);
            catch = Some(frag);
        }

        if self.looking_at("{/await}") {
            self.eat("{/await}")?;
        }

        Ok(TemplateNode::AwaitBlock(AwaitBlock {
            expression,
            pending,
            then,
            then_binding,
            catch,
            catch_binding,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_key_block(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#key")?;
        self.skip_whitespace();
        let expression = self.read_expression()?;
        self.eat("}")?;
        let body = self.parse_fragment()?;
        if self.looking_at("{/key}") {
            self.eat("{/key}")?;
        }
        Ok(TemplateNode::KeyBlock(KeyBlock {
            expression,
            body,
            span: Span::new(start, self.pos as u32),
        }))
    }

    fn parse_snippet_block(&mut self) -> Result<TemplateNode, OxcDiagnostic> {
        let start = self.pos as u32;
        self.eat("{#snippet")?;
        self.skip_whitespace();

        // Parse: name(params)
        let header = self.eat_until("}");
        self.eat("}")?;

        let (name, params) = if let Some(paren_idx) = header.find('(') {
            let name = header[..paren_idx].trim().to_string();
            let params_end = header.rfind(')').unwrap_or(header.len());
            let params = header[paren_idx + 1..params_end].to_string();
            (name, params)
        } else {
            (header.trim().to_string(), String::new())
        };

        let body = self.parse_fragment()?;

        if self.looking_at("{/snippet}") {
            self.eat("{/snippet}")?;
        }

        Ok(TemplateNode::SnippetBlock(SnippetBlock {
            name,
            params,
            body,
            span: Span::new(start, self.pos as u32),
        }))
    }
}

// ─── Utility functions ─────────────────────────────────────────────────────

/// Parse an `{#each}` header like `items as item, i (item.id)`.
fn parse_each_header(header: &str) -> (String, String, Option<String>, Option<String>) {
    let header = header.trim();

    // Split on " as "
    let (expression, rest) = if let Some(idx) = header.find(" as ") {
        (header[..idx].trim().to_string(), &header[idx + 4..])
    } else {
        return (header.to_string(), String::new(), None, None);
    };

    // Check for (key) at the end
    let (rest, key) = if let Some(paren_start) = rest.rfind('(') {
        let key = rest[paren_start + 1..].trim_end_matches(')').trim().to_string();
        (rest[..paren_start].trim(), Some(key))
    } else {
        (rest.trim(), None)
    };

    // Check for ", index" — but skip commas inside [] or {}
    let (context, index) = {
        let mut depth = 0i32;
        let mut comma_pos = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '[' | '{' | '(' => depth += 1,
                ']' | '}' | ')' => depth -= 1,
                ',' if depth == 0 => { comma_pos = Some(i); break; }
                _ => {}
            }
        }
        if let Some(comma_idx) = comma_pos {
            (
                rest[..comma_idx].trim().to_string(),
                Some(rest[comma_idx + 1..].trim().to_string()),
            )
        } else {
            (rest.trim().to_string(), None)
        }
    };

    (expression, context, index, key)
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

/// Parse a concatenated attribute value like `"hello {name}!"`.
fn parse_concat_value(value: &str) -> AttributeValue {
    let mut parts = Vec::new();
    let mut current_static = String::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            if !current_static.is_empty() {
                parts.push(AttributeValuePart::Static(
                    std::mem::take(&mut current_static),
                ));
            }
            let mut expr = String::new();
            let mut depth = 0;
            for ch in chars.by_ref() {
                if ch == '{' {
                    depth += 1;
                    expr.push(ch);
                } else if ch == '}' {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    expr.push(ch);
                } else {
                    expr.push(ch);
                }
            }
            parts.push(AttributeValuePart::Expression(expr));
        } else {
            current_static.push(ch);
        }
    }

    if !current_static.is_empty() {
        parts.push(AttributeValuePart::Static(current_static));
    }

    AttributeValue::Concat(parts)
}

/// Check if opening a new element should implicitly close the parent.
fn should_implicitly_close(parent: &str, child: &str) -> bool {
    match parent {
        "li" => child == "li",
        "dt" | "dd" => child == "dt" || child == "dd",
        "p" => matches!(child, "address" | "article" | "aside" | "blockquote" | "details" | "div" |
            "dl" | "fieldset" | "figcaption" | "figure" | "footer" | "form" | "h1" | "h2" |
            "h3" | "h4" | "h5" | "h6" | "header" | "hgroup" | "hr" | "main" | "menu" | "nav" |
            "ol" | "p" | "pre" | "section" | "table" | "ul"),
        "rt" | "rp" => child == "rt" || child == "rp",
        "optgroup" => child == "optgroup",
        "option" => child == "option" || child == "optgroup",
        "thead" => child == "tbody" || child == "tfoot",
        "tbody" => child == "tbody" || child == "tfoot",
        "tfoot" => child == "tbody",
        "tr" => child == "tr",
        "td" | "th" => child == "td" || child == "th" || child == "tr",
        _ => false,
    }
}

/// Check if an HTML element is a void element (self-closing by spec).
fn is_void_element(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text() {
        let result = parse_fragment("hello world").unwrap();
        assert_eq!(result.nodes.len(), 1);
        match &result.nodes[0] {
            TemplateNode::Text(t) => assert_eq!(t.data, "hello world"),
            _ => panic!("expected Text node"),
        }
    }

    #[test]
    fn test_parse_element() {
        let source = "<div>hello</div>";
        let result = parse_fragment(source).unwrap();
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
    fn test_parse_self_closing() {
        let source = "<br/>";
        let result = parse_fragment(source).unwrap();
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
        let result = parse_fragment(source).unwrap();
        match &result.nodes[0] {
            TemplateNode::MustacheTag(m) => assert_eq!(m.expression, "count"),
            _ => panic!("expected MustacheTag"),
        }
    }

    #[test]
    fn test_parse_if_block() {
        let source = "{#if visible}<p>hello</p>{/if}";
        let result = parse_fragment(source).unwrap();
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
        let result = parse_fragment(source).unwrap();
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
    fn test_parse_comment() {
        let source = "<!-- a comment -->";
        let result = parse_fragment(source).unwrap();
        match &result.nodes[0] {
            TemplateNode::Comment(c) => assert_eq!(c.data, " a comment "),
            _ => panic!("expected Comment"),
        }
    }

    #[test]
    fn test_parse_snippet_block() {
        let source = "{#snippet greeting(name)}<p>Hello {name}</p>{/snippet}";
        let result = parse_fragment(source).unwrap();
        match &result.nodes[0] {
            TemplateNode::SnippetBlock(s) => {
                assert_eq!(s.name, "greeting");
                assert_eq!(s.params, "name");
            }
            _ => panic!("expected SnippetBlock"),
        }
    }

    #[test]
    fn test_parse_render_tag() {
        let source = "{@render greeting('world')}";
        let result = parse_fragment(source).unwrap();
        match &result.nodes[0] {
            TemplateNode::RenderTag(r) => assert_eq!(r.expression, "greeting('world')"),
            _ => panic!("expected RenderTag"),
        }
    }

    #[test]
    fn test_parse_directive() {
        let source = r#"<button on:click|preventDefault={handler}>Click</button>"#;
        let result = parse_fragment(source).unwrap();
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
}
