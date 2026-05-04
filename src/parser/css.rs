//! Svelte-compatible CSS parser for the legacy AST format.
//!
//! The control flow mirrors Svelte's own CSS parser in
//! `packages/svelte/src/compiler/phases/1-parse/read/style.js`, then adapts
//! selector lists back to the legacy `Selector` shape expected by this crate's
//! serializers.
//!
//! Portions of this module are a Rust port of Svelte's CSS parser.
//! Copyright (c) 2016-2025 Svelte Contributors, MIT License.
//!
//! This module is the compatibility parser for Svelte's stylesheet AST shape.
//! `src/parser/selector.rs` remains the semantic selector walker for linter
//! rules that need typed selector components from the Servo `selectors` crate.

use serde_json::{json, Value};

/// Parse CSS content string into legacy AST children array.
/// `offset` is the byte position of the CSS content start in the original source.
pub fn parse_css_children(css: &str, offset: u32) -> Vec<Value> {
    parse_css(css, offset).children
}

pub fn parse_css(css: &str, offset: u32) -> CssParseResult {
    let mut parser = CssParser::new(css, offset);
    let children = parser.parse_rules();
    CssParseResult {
        children,
        errors: parser.errors,
        error_positions: parser.error_positions,
        position: parser.pos,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssParseErrorKind {
    EmptyDeclaration,
    ExpectedIdentifier,
    ExpectedToken,
    InvalidDeclaration,
    InvalidSelector,
    Recovery,
    UnclosedComment,
    UnexpectedEof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssParseError {
    pub position: usize,
    pub kind: CssParseErrorKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CssParseResult {
    pub children: Vec<Value>,
    /// Structured parser diagnostics. Positions are relative to the CSS input.
    pub errors: Vec<CssParseError>,
    /// Legacy compatibility positions. Prefer `errors` for new code.
    pub error_positions: Vec<usize>,
    /// Final parser byte position, relative to the CSS input.
    pub position: usize,
}

pub struct CssParser<'a> {
    pub source: &'a str,
    pub pos: usize,
    pub offset: u32,
    /// Structured parser diagnostics. Positions are relative to `source`.
    pub errors: Vec<CssParseError>,
    /// Positions where parsing failed and recovery was needed.
    ///
    /// Kept for existing linter code; prefer `errors` for new logic.
    pub error_positions: Vec<usize>,
}

impl<'a> CssParser<'a> {
    pub fn new(source: &'a str, offset: u32) -> Self {
        Self {
            source,
            pos: 0,
            offset,
            errors: Vec::new(),
            error_positions: Vec::new(),
        }
    }

    pub fn parse_rules(&mut self) -> Vec<Value> {
        self.read_body(|parser| parser.pos >= parser.source.len())
    }

    fn read_body<F>(&mut self, finished: F) -> Vec<Value>
    where
        F: Fn(&Self) -> bool,
    {
        let mut children = Vec::new();

        loop {
            self.allow_comment_or_whitespace();
            if finished(self) {
                break;
            }

            let before = self.pos;
            let child = if self.match_str("@") {
                self.read_at_rule()
            } else if self.match_str("$") {
                self.read_declaration()
            } else {
                self.read_rule()
            };

            if let Some(child) = child {
                children.push(child);
            }

            if self.pos == before {
                self.error_at(self.pos, CssParseErrorKind::Recovery);
                self.bump_char();
            }
        }

        children
    }

    fn read_at_rule(&mut self) -> Option<Value> {
        let start = self.pos;
        self.expect("@")?;

        let name = self.read_identifier();
        self.allow_whitespace();
        let prelude = self.read_value().unwrap_or_default();

        let block = if self.match_str("{") {
            self.read_block()
        } else {
            if !self.eat(";") {
                self.error_at(self.pos, CssParseErrorKind::ExpectedToken);
            }
            None
        };

        Some(json!({
            "type": "Atrule",
            "start": self.abs(start),
            "end": self.abs(self.pos),
            "name": name,
            "prelude": prelude,
            "block": block
        }))
    }

    fn read_rule(&mut self) -> Option<Value> {
        let start = self.pos;
        let prelude = self.read_selector_list_legacy(false)?;
        let block = self.read_block()?;

        Some(json!({
            "type": "Rule",
            "prelude": prelude,
            "block": block,
            "start": self.abs(start),
            "end": self.abs(self.pos)
        }))
    }

    fn read_selector_list_legacy(&mut self, inside_pseudo_class: bool) -> Option<Value> {
        let modern = self.read_selector_list(inside_pseudo_class)?;
        Some(modern_selector_list_to_legacy(modern))
    }

    fn read_selector_list(&mut self, inside_pseudo_class: bool) -> Option<Value> {
        let mut children = Vec::new();

        self.allow_comment_or_whitespace();
        let start = self.pos;

        while self.pos < self.source.len() {
            let selector = self.read_selector(inside_pseudo_class)?;
            children.push(selector);

            let end = self.pos;
            self.allow_comment_or_whitespace();

            if if inside_pseudo_class {
                self.match_str(")")
            } else {
                self.match_str("{")
            } {
                return Some(json!({
                    "type": "SelectorList",
                    "start": self.abs(start),
                    "end": self.abs(end),
                    "children": children
                }));
            }

            if !self.eat(",") {
                self.error_at(self.pos, CssParseErrorKind::InvalidSelector);
                return Some(json!({
                    "type": "SelectorList",
                    "start": self.abs(start),
                    "end": self.abs(end),
                    "children": children
                }));
            }
            self.allow_comment_or_whitespace();
        }

        self.error_at(self.source.len(), CssParseErrorKind::UnexpectedEof);
        if children.is_empty() {
            None
        } else {
            Some(json!({
                "type": "SelectorList",
                "start": self.abs(start),
                "end": self.abs(self.pos),
                "children": children
            }))
        }
    }

    fn read_selector(&mut self, inside_pseudo_class: bool) -> Option<Value> {
        let list_start = self.pos;
        let mut children = Vec::new();
        let mut relative_selector = RelativeSelectorBuilder::new(None, self.pos);

        while self.pos < self.source.len() {
            let start = self.pos;

            if self.eat("&") {
                relative_selector.selectors.push(json!({
                    "type": "NestingSelector",
                    "name": "&",
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if self.eat("*") {
                let mut name = "*".to_string();

                if self.eat("|") {
                    name = self.read_identifier();
                }

                relative_selector.selectors.push(json!({
                    "type": "TypeSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if self.eat("#") {
                let name = self.read_identifier();
                relative_selector.selectors.push(json!({
                    "type": "IdSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if self.eat(".") {
                let name = self.read_identifier();
                relative_selector.selectors.push(json!({
                    "type": "ClassSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if self.eat("%") {
                let name = self.read_identifier();
                relative_selector.selectors.push(json!({
                    "type": "PlaceholderSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if self.eat("::") {
                let name = self.read_identifier();
                relative_selector.selectors.push(json!({
                    "type": "PseudoElementSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));

                if self.eat("(") {
                    let _ = self.read_selector_list(true);
                    self.expect(")");
                }
            } else if self.eat(":") {
                let name = self.read_identifier();
                let mut args = Value::Null;

                if self.eat("(") {
                    args = self.read_selector_list(true).unwrap_or_else(|| {
                        json!({
                            "type": "SelectorList",
                            "start": self.abs(self.pos),
                            "end": self.abs(self.pos),
                            "children": []
                        })
                    });
                    self.expect(")");
                }

                relative_selector.selectors.push(json!({
                    "type": "PseudoClassSelector",
                    "name": name,
                    "args": args,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if self.eat("[") {
                self.allow_comment_or_whitespace();
                let name = self.read_attribute_name();
                self.allow_whitespace();

                let matcher = self.read_matcher();
                let mut value = None;

                if matcher.is_some() {
                    self.allow_whitespace();
                    value = Some(self.read_attribute_value());
                }

                self.allow_whitespace();
                let flags = self.read_attribute_flags();
                self.allow_whitespace();
                self.expect("]");

                relative_selector.selectors.push(json!({
                    "type": "AttributeSelector",
                    "name": name,
                    "matcher": matcher,
                    "value": value,
                    "flags": flags,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }));
            } else if inside_pseudo_class {
                if let Some((value, end)) = self.read_nth() {
                    relative_selector.selectors.push(json!({
                        "type": "Nth",
                        "value": value,
                        "start": self.abs(start),
                        "end": self.abs(end)
                    }));
                    self.pos = end;
                } else if let Some((value, end)) = self.read_percentage() {
                    relative_selector.selectors.push(json!({
                        "type": "Percentage",
                        "value": value,
                        "start": self.abs(start),
                        "end": self.abs(end)
                    }));
                    self.pos = end;
                } else if !self.match_combinator() {
                    self.read_type_selector(start, &mut relative_selector);
                }
            } else if let Some((value, end)) = self.read_percentage() {
                relative_selector.selectors.push(json!({
                    "type": "Percentage",
                    "value": value,
                    "start": self.abs(start),
                    "end": self.abs(end)
                }));
                self.pos = end;
            } else if !self.match_combinator() {
                self.read_type_selector(start, &mut relative_selector);
            }

            let index = self.pos;
            self.allow_comment_or_whitespace();

            if self.match_str(",")
                || if inside_pseudo_class {
                    self.match_str(")")
                } else {
                    self.match_str("{")
                }
            {
                self.pos = index;
                relative_selector.end = index;
                if !relative_selector.selectors.is_empty() {
                    children.push(relative_selector.finish(self.offset));
                }

                return Some(json!({
                    "type": "ComplexSelector",
                    "start": self.abs(list_start),
                    "end": self.abs(index),
                    "children": children
                }));
            }

            self.pos = index;
            if let Some(combinator) = self.read_combinator() {
                if !relative_selector.selectors.is_empty() {
                    relative_selector.end = index;
                    children.push(relative_selector.finish(self.offset));
                }

                let combinator_start = combinator
                    .get("start")
                    .and_then(Value::as_u64)
                    .map(|n| (n as u32).saturating_sub(self.offset) as usize)
                    .unwrap_or(self.pos);
                relative_selector =
                    RelativeSelectorBuilder::new(Some(combinator), combinator_start);

                self.allow_whitespace();

                if self.match_str(",")
                    || if inside_pseudo_class {
                        self.match_str(")")
                    } else {
                        self.match_str("{")
                    }
                {
                    self.error_at(self.pos, CssParseErrorKind::InvalidSelector);
                }
            }
        }

        self.error_at(self.source.len(), CssParseErrorKind::UnexpectedEof);
        None
    }

    fn read_type_selector(
        &mut self,
        start: usize,
        relative_selector: &mut RelativeSelectorBuilder,
    ) {
        let name = if self.eat("|") {
            self.read_identifier()
        } else {
            let mut name = self.read_identifier();
            if self.eat("|") {
                name = self.read_identifier();
            }
            name
        };

        relative_selector.selectors.push(json!({
            "type": "TypeSelector",
            "name": name,
            "start": self.abs(start),
            "end": self.abs(self.pos)
        }));
    }

    fn read_attribute_name(&mut self) -> String {
        let start = self.pos;

        if self.eat("*") {
            if self.eat("|") {
                return self.read_identifier();
            }
            self.error_at(start, CssParseErrorKind::ExpectedIdentifier);
            return "*".to_string();
        }

        if self.eat("|") {
            return self.read_identifier();
        }

        let mut name = self.read_identifier();
        if self.match_str("|") && !self.match_str("|=") {
            self.pos += 1;
            name = self.read_identifier();
        }
        name
    }

    fn read_block(&mut self) -> Option<Value> {
        let start = self.pos;
        self.expect("{")?;

        let mut children = Vec::new();

        while self.pos < self.source.len() {
            self.allow_comment_or_whitespace();

            if self.match_str("}") {
                break;
            }

            let before = self.pos;
            if let Some(child) = self.read_block_item() {
                children.push(child);
            }

            if self.pos == before {
                self.error_at(self.pos, CssParseErrorKind::Recovery);
                self.bump_char();
            }
        }

        self.expect("}");

        Some(json!({
            "type": "Block",
            "start": self.abs(start),
            "end": self.abs(self.pos),
            "children": children
        }))
    }

    fn read_block_item(&mut self) -> Option<Value> {
        if self.match_str("@") {
            return self.read_at_rule();
        }

        let start = self.pos;
        let error_len = self.error_positions.len();
        let errors_len = self.errors.len();
        let _ = self.read_value();
        let is_nested_rule = self.match_str("{");
        self.pos = start;
        self.error_positions.truncate(error_len);
        self.errors.truncate(errors_len);

        if is_nested_rule {
            self.read_rule()
        } else {
            self.read_declaration()
        }
    }

    fn read_declaration(&mut self) -> Option<Value> {
        let start = self.pos;
        let property_start = self.pos;
        let property = self.read_until_whitespace_or_colon();
        self.allow_whitespace();

        if !self.eat(":") {
            self.error_at(property_start, CssParseErrorKind::InvalidDeclaration);
            let _ = self.read_value();
            if !self.match_str("}") {
                self.eat(";");
            }

            return Some(json!({
                "type": "Declaration",
                "start": self.abs(start),
                "end": self.abs(self.pos),
                "property": property,
                "value": ""
            }));
        }

        let value_start = self.pos;
        self.allow_whitespace();

        let value = self.read_value().unwrap_or_default();

        if value.is_empty() && !property.starts_with("--") {
            self.error_at(value_start, CssParseErrorKind::EmptyDeclaration);
        }

        let end = self.pos;

        if !self.match_str("}") {
            self.expect(";");
        }

        Some(json!({
            "type": "Declaration",
            "start": self.abs(start),
            "end": self.abs(end),
            "property": property,
            "value": value
        }))
    }

    fn read_value(&mut self) -> Option<String> {
        let mut value = String::new();
        let mut escaped = false;
        let mut in_url = false;
        let mut quote_mark = None;

        while self.pos < self.source.len() {
            let ch = self.current_char()?;

            if escaped {
                value.push('\\');
                value.push(ch);
                escaped = false;
                self.bump_char();
                continue;
            } else if ch == '\\' {
                escaped = true;
                self.bump_char();
                continue;
            } else if Some(ch) == quote_mark {
                quote_mark = None;
            } else if ch == ')' {
                in_url = false;
            } else if quote_mark.is_none() && (ch == '"' || ch == '\'') {
                quote_mark = Some(ch);
            } else if ch == '(' && value.ends_with("url") {
                in_url = true;
            } else if (ch == ';' || ch == '{' || ch == '}') && !in_url && quote_mark.is_none() {
                return Some(value.trim().to_string());
            } else if ch == '/'
                && !in_url
                && quote_mark.is_none()
                && self.source[self.pos..].starts_with("/*")
            {
                self.pos += 2;
                while self.pos < self.source.len() {
                    if self.source[self.pos..].starts_with("*/") {
                        self.pos += 2;
                        break;
                    }
                    self.bump_char();
                }
                continue;
            }

            value.push(ch);
            self.bump_char();
        }

        self.error_at(self.source.len(), CssParseErrorKind::UnexpectedEof);
        None
    }

    fn read_attribute_value(&mut self) -> String {
        let mut value = String::new();
        let mut escaped = false;
        let quote_mark = if self.eat("\"") {
            Some('"')
        } else if self.eat("'") {
            Some('\'')
        } else {
            None
        };

        while self.pos < self.source.len() {
            let Some(ch) = self.current_char() else {
                break;
            };

            if escaped {
                value.push('\\');
                value.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if quote_mark.map_or(ch.is_whitespace() || ch == ']', |quote| ch == quote) {
                if let Some(quote) = quote_mark {
                    let quote_str = quote.to_string();
                    self.expect(&quote_str);
                }
                return value.trim().to_string();
            } else {
                value.push(ch);
            }

            self.bump_char();
        }

        self.error_at(self.source.len(), CssParseErrorKind::UnexpectedEof);
        value.trim().to_string()
    }

    fn read_identifier(&mut self) -> String {
        let start = self.pos;
        let mut identifier = String::new();

        if self.starts_like_leading_hyphen_or_digit() {
            self.error_at(start, CssParseErrorKind::ExpectedIdentifier);
        }

        while self.pos < self.source.len() {
            let Some(ch) = self.current_char() else {
                break;
            };

            if ch == '\\' {
                self.bump_char();
                if let Some(decoded) = self.read_escape_sequence() {
                    identifier.push(decoded);
                } else if let Some(next) = self.current_char() {
                    identifier.push('\\');
                    identifier.push(next);
                    self.bump_char();
                } else {
                    identifier.push('\\');
                }
            } else if is_valid_identifier_char(ch) {
                identifier.push(ch);
                self.bump_char();
            } else {
                break;
            }
        }

        if identifier.is_empty() {
            self.error_at(start, CssParseErrorKind::ExpectedIdentifier);
            if self.pos == start {
                self.bump_char();
            }
        }

        identifier
    }

    fn read_escape_sequence(&mut self) -> Option<char> {
        let start = self.pos;
        let mut end = self.pos;
        let mut digits = 0;

        while end < self.source.len() && digits < 6 {
            let ch = self.source[end..].chars().next()?;
            if !ch.is_ascii_hexdigit() {
                break;
            }
            end += ch.len_utf8();
            digits += 1;
        }

        if digits == 0 {
            return None;
        }

        let code = u32::from_str_radix(&self.source[start..end], 16).ok()?;
        self.pos = end;

        if self.source[self.pos..].starts_with("\r\n") {
            self.pos += 2;
        } else if self.current_char().is_some_and(is_css_whitespace) {
            self.bump_char();
        }

        char::from_u32(code)
    }

    fn read_until_whitespace_or_colon(&mut self) -> String {
        let start = self.pos;

        while self.pos < self.source.len() {
            let Some(ch) = self.current_char() else {
                break;
            };
            if ch == ':' || is_css_whitespace(ch) {
                break;
            }
            self.bump_char();
        }

        self.source[start..self.pos].to_string()
    }

    fn read_matcher(&mut self) -> Option<String> {
        let rest = &self.source[self.pos..];
        let bytes = rest.as_bytes();
        if bytes.first() == Some(&b'=') {
            self.pos += 1;
            return Some("=".to_string());
        }
        if bytes.len() >= 2
            && matches!(bytes[0], b'~' | b'^' | b'$' | b'*' | b'|')
            && bytes[1] == b'='
        {
            self.pos += 2;
            return Some(rest[..2].to_string());
        }
        None
    }

    fn read_attribute_flags(&mut self) -> Option<String> {
        let start = self.pos;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch.is_ascii_alphabetic() {
                self.pos += 1;
            } else {
                break;
            }
        }

        if self.pos == start {
            None
        } else {
            Some(self.source[start..self.pos].to_string())
        }
    }

    fn read_percentage(&self) -> Option<(String, usize)> {
        let mut pos = self.pos;
        let start = pos;

        while pos < self.source.len() && self.source.as_bytes()[pos].is_ascii_digit() {
            pos += 1;
        }

        if pos < self.source.len() && self.source.as_bytes()[pos] == b'.' {
            let dot = pos;
            pos += 1;
            while pos < self.source.len() && self.source.as_bytes()[pos].is_ascii_digit() {
                pos += 1;
            }
            if pos == dot + 1 {
                pos = dot;
            }
        }

        if pos == start || pos >= self.source.len() || self.source.as_bytes()[pos] != b'%' {
            return None;
        }

        pos += 1;
        Some((self.source[start..pos].to_string(), pos))
    }

    fn read_nth(&self) -> Option<(String, usize)> {
        let start = self.pos;
        let rest = &self.source[start..];

        for word in ["even", "odd"] {
            if rest.starts_with(word) {
                let end = start + word.len();
                if self.nth_has_required_suffix(end) {
                    return Some((word.to_string(), end));
                }
            }
        }

        let mut pos = start;
        if self.byte_at(pos).is_some_and(|b| b == b'+' || b == b'-') {
            pos += 1;
        }

        let digit_start = pos;
        while self.byte_at(pos).is_some_and(|b| b.is_ascii_digit()) {
            pos += 1;
        }
        let digits_before_n = pos > digit_start;

        let mut saw_n = false;
        if self.byte_at(pos).is_some_and(|b| b == b'n' || b == b'N') {
            saw_n = true;
            pos += 1;

            let after_n_ws = self.skip_css_whitespace_from(pos);
            if self
                .byte_at(after_n_ws)
                .is_some_and(|b| b == b'+' || b == b'-')
            {
                let sign = after_n_ws + 1;
                let digits = self.skip_css_whitespace_from(sign);
                let digit_start = digits;
                let mut digit_end = digit_start;
                while self.byte_at(digit_end).is_some_and(|b| b.is_ascii_digit()) {
                    digit_end += 1;
                }
                if digit_end == digit_start {
                    return None;
                }
                pos = digit_end;
            }
        }

        if !saw_n && !digits_before_n {
            return None;
        }

        if self.nth_has_required_suffix(pos) {
            Some((
                self.source[start..self.nth_value_end(pos)].to_string(),
                self.nth_value_end(pos),
            ))
        } else {
            None
        }
    }

    fn nth_has_required_suffix(&self, pos: usize) -> bool {
        let ws = self.skip_css_whitespace_from(pos);
        if self.byte_at(ws).is_some_and(|b| b == b',' || b == b')') {
            return true;
        }

        if ws > pos && self.source[ws..].starts_with("of") {
            let after_of = ws + 2;
            if after_of < self.source.len() {
                return self
                    .current_char_at(after_of)
                    .is_some_and(is_css_whitespace);
            }
        }

        false
    }

    fn nth_value_end(&self, pos: usize) -> usize {
        let ws = self.skip_css_whitespace_from(pos);
        if ws > pos && self.source[ws..].starts_with("of") {
            let mut end = ws + 2;
            if end < self.source.len() && self.current_char_at(end).is_some_and(is_css_whitespace) {
                while end < self.source.len()
                    && self.current_char_at(end).is_some_and(is_css_whitespace)
                {
                    end += self.current_char_at(end).map(char::len_utf8).unwrap_or(1);
                }
                return end;
            }
        }
        pos
    }

    fn read_combinator(&mut self) -> Option<Value> {
        let start = self.pos;
        self.allow_comment_or_whitespace();

        let index = self.pos;
        let name = if self.source[self.pos..].starts_with("||") {
            self.pos += 2;
            Some("||".to_string())
        } else if let Some(ch) = self.current_char() {
            if matches!(ch, '+' | '~' | '>') {
                self.bump_char();
                Some(ch.to_string())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(name) = name {
            let end = self.pos;
            self.allow_comment_or_whitespace();
            return Some(json!({
                "type": "Combinator",
                "name": name,
                "start": self.abs(index),
                "end": self.abs(end)
            }));
        }

        if self.pos != start {
            return Some(json!({
                "type": "Combinator",
                "name": " ",
                "start": self.abs(start),
                "end": self.abs(self.pos)
            }));
        }

        None
    }

    fn match_combinator(&self) -> bool {
        self.source[self.pos..].starts_with("||")
            || self
                .current_char()
                .is_some_and(|ch| matches!(ch, '+' | '~' | '>'))
    }

    fn allow_comment_or_whitespace(&mut self) {
        loop {
            let before = self.pos;
            self.allow_whitespace();

            while self.source[self.pos..].starts_with("/*")
                || self.source[self.pos..].starts_with("<!--")
                || self.source[self.pos..].starts_with("//")
            {
                if self.source[self.pos..].starts_with("/*") {
                    self.pos += 2;
                    if let Some(end) = self.source[self.pos..].find("*/") {
                        self.pos += end + 2;
                    } else {
                        self.error_at(
                            self.pos.saturating_sub(2),
                            CssParseErrorKind::UnclosedComment,
                        );
                        self.pos = self.source.len();
                    }
                } else if self.source[self.pos..].starts_with("<!--") {
                    self.pos += 4;
                    if let Some(end) = self.source[self.pos..].find("-->") {
                        self.pos += end + 3;
                    } else {
                        self.error_at(
                            self.pos.saturating_sub(4),
                            CssParseErrorKind::UnclosedComment,
                        );
                        self.pos = self.source.len();
                    }
                } else {
                    self.pos += 2;
                    while self.pos < self.source.len() {
                        let Some(ch) = self.current_char() else {
                            break;
                        };
                        if ch == '\n' || ch == '\r' {
                            break;
                        }
                        self.bump_char();
                    }
                }

                self.allow_whitespace();
            }

            if self.pos == before {
                break;
            }
        }
    }

    fn allow_whitespace(&mut self) {
        while self.pos < self.source.len() {
            let Some(ch) = self.current_char() else {
                break;
            };
            if !is_css_whitespace(ch) {
                break;
            }
            self.bump_char();
        }
    }

    fn starts_like_leading_hyphen_or_digit(&self) -> bool {
        let rest = &self.source[self.pos..];
        let bytes = rest.as_bytes();
        bytes.first().is_some_and(u8::is_ascii_digit)
            || (bytes.first() == Some(&b'-') && bytes.get(1).is_some_and(u8::is_ascii_digit))
    }

    fn skip_css_whitespace_from(&self, mut pos: usize) -> usize {
        while pos < self.source.len() {
            let Some(ch) = self.current_char_at(pos) else {
                break;
            };
            if !is_css_whitespace(ch) {
                break;
            }
            pos += ch.len_utf8();
        }
        pos
    }

    fn expect(&mut self, str: &str) -> Option<()> {
        if self.eat(str) {
            Some(())
        } else {
            self.error_at(self.pos, CssParseErrorKind::ExpectedToken);
            None
        }
    }

    fn eat(&mut self, str: &str) -> bool {
        if self.match_str(str) {
            self.pos += str.len();
            true
        } else {
            false
        }
    }

    fn match_str(&self, str: &str) -> bool {
        self.source[self.pos..].starts_with(str)
    }

    fn abs(&self, pos: usize) -> u32 {
        self.offset + pos as u32
    }

    fn byte_at(&self, pos: usize) -> Option<u8> {
        self.source.as_bytes().get(pos).copied()
    }

    fn current_char(&self) -> Option<char> {
        self.current_char_at(self.pos)
    }

    fn current_char_at(&self, pos: usize) -> Option<char> {
        self.source.get(pos..)?.chars().next()
    }

    fn bump_char(&mut self) {
        if let Some(ch) = self.current_char() {
            self.pos += ch.len_utf8();
        } else {
            self.pos = self.source.len();
        }
    }

    fn error_at(&mut self, pos: usize, kind: CssParseErrorKind) {
        if self.error_positions.last().copied() != Some(pos) {
            self.error_positions.push(pos);
        }
        if self
            .errors
            .last()
            .is_none_or(|error| error.position != pos || error.kind != kind)
        {
            self.errors.push(CssParseError {
                position: pos,
                kind,
            });
        }
    }
}

struct RelativeSelectorBuilder {
    combinator: Option<Value>,
    selectors: Vec<Value>,
    start: usize,
    end: usize,
}

impl RelativeSelectorBuilder {
    fn new(combinator: Option<Value>, start: usize) -> Self {
        Self {
            combinator,
            selectors: Vec::new(),
            start,
            end: start,
        }
    }

    fn finish(self, offset: u32) -> Value {
        json!({
            "type": "RelativeSelector",
            "combinator": self.combinator,
            "selectors": self.selectors,
            "start": offset + self.start as u32,
            "end": offset + self.end as u32
        })
    }
}

fn modern_selector_list_to_legacy(selector_list: Value) -> Value {
    let start = selector_list
        .get("start")
        .cloned()
        .unwrap_or_else(|| json!(0));
    let end = selector_list
        .get("end")
        .cloned()
        .unwrap_or_else(|| json!(0));

    let children = selector_list
        .get("children")
        .and_then(Value::as_array)
        .map(|selectors| {
            selectors
                .iter()
                .map(modern_complex_selector_to_legacy)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "type": "SelectorList",
        "start": start,
        "end": end,
        "_full_end": end,
        "children": children
    })
}

fn modern_complex_selector_to_legacy(complex: &Value) -> Value {
    let start = complex.get("start").cloned().unwrap_or_else(|| json!(0));
    let full_end = complex.get("end").cloned().unwrap_or_else(|| json!(0));
    let mut children = Vec::new();

    if let Some(relative_selectors) = complex.get("children").and_then(Value::as_array) {
        for relative in relative_selectors {
            if let Some(combinator) = relative.get("combinator") {
                if !combinator.is_null() {
                    children.push(combinator.clone());
                }
            }

            if let Some(selectors) = relative.get("selectors").and_then(Value::as_array) {
                children.extend(selectors.iter().map(simple_selector_to_legacy));
            }
        }
    }

    let end = children
        .last()
        .and_then(|child| child.get("end"))
        .cloned()
        .unwrap_or_else(|| full_end.clone());

    json!({
        "type": "Selector",
        "start": start,
        "end": end,
        "_full_end": full_end,
        "children": children
    })
}

fn simple_selector_to_legacy(selector: &Value) -> Value {
    let mut selector = selector.clone();

    if let Some(obj) = selector.as_object_mut() {
        if let Some(args) = obj.get_mut("args") {
            if !args.is_null() {
                *args = modern_selector_list_to_legacy(args.clone());
            }
        }
    }

    selector
}

fn is_valid_identifier_char(ch: char) -> bool {
    ch as u32 >= 160 || ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn is_css_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2028}'
            | '\u{2029}'
            | '\u{202f}'
            | '\u{205f}'
            | '\u{3000}'
            | '\u{feff}'
    ) || ('\u{2000}'..='\u{200a}').contains(&ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_rules_and_line_comments() {
        let mut parser = CssParser::new(
            ".container { .item { // scss-style comment\n color: red; } }",
            0,
        );
        let children = parser.parse_rules();
        assert!(parser.error_positions.is_empty());
        assert_eq!(children.len(), 1);
        let block_children = children[0]["block"]["children"].as_array().unwrap();
        assert_eq!(block_children[0]["type"], "Rule");
    }

    #[test]
    fn parses_scss_variables_and_placeholders_leniently() {
        let mut parser = CssParser::new("$brand: red; %button { color: $brand; }", 0);
        let children = parser.parse_rules();
        assert!(parser.errors.is_empty());

        assert_eq!(children[0]["type"], "Declaration");
        assert_eq!(children[0]["property"], "$brand");

        let selector = &children[1]["prelude"]["children"][0]["children"][0];
        assert_eq!(selector["type"], "PlaceholderSelector");
        assert_eq!(selector["name"], "button");
    }

    #[test]
    fn parses_attribute_selector_parts() {
        let children = parse_css_children(r#"[data-foo="bar" i] { color: red; }"#, 0);
        let selector = &children[0]["prelude"]["children"][0]["children"][0];
        assert_eq!(selector["type"], "AttributeSelector");
        assert_eq!(selector["name"], "data-foo");
        assert_eq!(selector["matcher"], "=");
        assert_eq!(selector["value"], "bar");
        assert_eq!(selector["flags"], "i");
    }

    #[test]
    fn parses_attribute_selector_namespaces_without_breaking_pipe_matcher() {
        let mut parser = CssParser::new(
            r#"[xlink|href] { color: red; } [data-foo|="bar"] { color: blue; }"#,
            0,
        );
        let children = parser.parse_rules();
        assert!(parser.errors.is_empty());

        let namespaced = &children[0]["prelude"]["children"][0]["children"][0];
        assert_eq!(namespaced["type"], "AttributeSelector");
        assert_eq!(namespaced["name"], "href");
        assert!(namespaced["matcher"].is_null());

        let pipe_matcher = &children[1]["prelude"]["children"][0]["children"][0];
        assert_eq!(pipe_matcher["name"], "data-foo");
        assert_eq!(pipe_matcher["matcher"], "|=");
        assert_eq!(pipe_matcher["value"], "bar");
    }

    #[test]
    fn parses_namespaced_type_selectors() {
        let mut parser = CssParser::new("svg|a, |button, *|section { color: red; }", 0);
        let children = parser.parse_rules();
        assert!(parser.errors.is_empty());

        let selectors = children[0]["prelude"]["children"].as_array().unwrap();
        assert_eq!(selectors[0]["children"][0]["name"], "a");
        assert_eq!(selectors[1]["children"][0]["name"], "button");
        assert_eq!(selectors[2]["children"][0]["name"], "section");
    }

    #[test]
    fn parses_descendant_combinator() {
        let children = parse_css_children("div span { color: red; }", 0);
        let selector_children = children[0]["prelude"]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert_eq!(selector_children[1]["type"], "Combinator");
        assert_eq!(selector_children[1]["name"], " ");
    }

    #[test]
    fn parses_commented_explicit_combinator() {
        let mut parser = CssParser::new("a /* before */ > /* after */ b { color: red; }", 0);
        let children = parser.parse_rules();
        assert!(parser.errors.is_empty(), "{:?}", parser.errors);

        let selector_children = children[0]["prelude"]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert_eq!(selector_children[1]["type"], "Combinator");
        assert_eq!(selector_children[1]["name"], ">");
        assert_eq!(selector_children[2]["type"], "TypeSelector");
        assert_eq!(selector_children[2]["name"], "b");
    }

    #[test]
    fn parses_nth_of_selector() {
        let children = parse_css_children("h1:nth-child(-n + 3 of li.important) {}", 0);
        let pseudo = &children[0]["prelude"]["children"][0]["children"][1];
        let args = &pseudo["args"]["children"][0]["children"];
        assert_eq!(args[0]["type"], "Nth");
        assert_eq!(args[0]["value"], "-n + 3 of ");
        assert_eq!(args[1]["type"], "TypeSelector");
        assert_eq!(args[2]["type"], "ClassSelector");
    }

    #[test]
    fn reports_missing_declaration_colon() {
        let mut parser = CssParser::new(".x { color red; }", 0);
        let _ = parser.parse_rules();
        assert!(!parser.error_positions.is_empty());
        assert_eq!(parser.errors[0].kind, CssParseErrorKind::InvalidDeclaration);
    }

    #[test]
    fn parse_css_returns_children_and_structured_errors() {
        let result = parse_css(".x { color red; }", 10);
        assert_eq!(result.children.len(), 1);
        assert_eq!(
            result.errors.first().map(|error| error.kind),
            Some(CssParseErrorKind::InvalidDeclaration)
        );
        assert_eq!(result.error_positions.first().copied(), Some(5));
        assert_eq!(result.position, ".x { color red; }".len());
    }

    #[test]
    fn parses_at_rules_and_imports() {
        let children = parse_css_children(
            "@import 'foo.css'; @media (min-width: 800px) { div { color: red; } }",
            0,
        );

        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["type"], "Atrule");
        assert_eq!(children[0]["name"], "import");
        assert!(children[0]["block"].is_null());
        assert_eq!(children[1]["type"], "Atrule");
        assert_eq!(children[1]["name"], "media");
        assert_eq!(children[1]["block"]["children"][0]["type"], "Rule");
    }

    #[test]
    fn skips_css_and_html_comments() {
        let children = parse_css_children("<!-- ignored --> /* ignored */ div { color: red; }", 0);
        assert_eq!(children.len(), 1);
        assert_eq!(children[0]["type"], "Rule");
        assert_eq!(
            children[0]["prelude"]["children"][0]["children"][0]["name"],
            "div"
        );
    }

    #[test]
    fn parses_keyframe_blocks() {
        let children = parse_css_children(
            "@keyframes fade { from { opacity: 0; } 50% { opacity: .5; } }",
            0,
        );
        let block_children = children[0]["block"]["children"].as_array().unwrap();

        assert_eq!(children[0]["name"], "keyframes");
        assert_eq!(block_children[0]["type"], "Rule");
        assert_eq!(
            block_children[1]["prelude"]["children"][0]["children"][0]["type"],
            "Percentage"
        );
    }

    #[test]
    fn preserves_escaped_url_value() {
        let children = parse_css_children(r#"div { background: url('./example.png?\''); }"#, 0);
        assert_eq!(
            children[0]["block"]["children"][0]["value"],
            r#"url('./example.png?\'')"#
        );
    }

    #[test]
    fn parses_nesting_selector() {
        let children = parse_css_children(".foo { & > .bar { color: red; } }", 0);
        let nested = &children[0]["block"]["children"][0];
        let selector_children = nested["prelude"]["children"][0]["children"]
            .as_array()
            .unwrap();

        assert_eq!(nested["type"], "Rule");
        assert_eq!(selector_children[0]["type"], "NestingSelector");
        assert_eq!(selector_children[1]["type"], "Combinator");
    }

    #[test]
    fn parses_global_pseudo_and_block_shapes() {
        let mut parser = CssParser::new(":global(.foo) {} :global { .bar {} }", 0);
        let children = parser.parse_rules();
        assert!(parser.errors.is_empty());

        let inline_global = &children[0]["prelude"]["children"][0]["children"][0];
        assert_eq!(inline_global["type"], "PseudoClassSelector");
        assert_eq!(inline_global["name"], "global");
        assert_eq!(
            inline_global["args"]["children"][0]["children"][0]["name"],
            "foo"
        );

        let block_global = &children[1];
        assert_eq!(
            block_global["prelude"]["children"][0]["children"][0]["name"],
            "global"
        );
        assert_eq!(block_global["block"]["children"][0]["type"], "Rule");
        assert_eq!(
            block_global["block"]["children"][0]["prelude"]["children"][0]["children"][0]["name"],
            "bar"
        );
    }
}
