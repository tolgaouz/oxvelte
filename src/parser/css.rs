//! Basic CSS parser for Svelte's legacy AST format.
//!
//! Parses CSS content into Rule, SelectorList, Selector, TypeSelector,
//! Block, Declaration nodes matching the Svelte compiler's legacy output.

use serde_json::{json, Value};

/// Parse CSS content string into legacy AST children array.
/// `offset` is the byte position of the CSS content start in the original source.
pub fn parse_css_children(css: &str, offset: u32) -> Vec<Value> {
    let mut parser = CssParser::new(css, offset);
    parser.parse_rules()
}

struct CssParser<'a> {
    source: &'a str,
    pos: usize,
    offset: u32,
}

impl<'a> CssParser<'a> {
    fn new(source: &'a str, offset: u32) -> Self {
        Self { source, pos: 0, offset }
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.source.len() && self.source.as_bytes()[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn skip_comments(&mut self) {
        while self.pos + 1 < self.source.len() {
            if &self.source[self.pos..self.pos + 2] == "/*" {
                if let Some(end) = self.source[self.pos + 2..].find("*/") {
                    self.pos += end + 4;
                } else {
                    self.pos = self.source.len();
                }
            } else {
                break;
            }
            self.skip_whitespace();
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            let before = self.pos;
            self.skip_whitespace();
            self.skip_comments();
            if self.pos == before {
                break;
            }
        }
    }

    fn abs(&self, pos: usize) -> u32 {
        self.offset + pos as u32
    }

    fn parse_rules(&mut self) -> Vec<Value> {
        let mut rules = Vec::new();
        self.skip_ws_and_comments();

        while self.pos < self.source.len() {
            if self.source[self.pos..].starts_with('}') {
                break;
            }

            if self.source[self.pos..].starts_with('@') {
                if let Some(atrule) = self.parse_atrule() {
                    rules.push(atrule);
                }
            } else {
                if let Some(rule) = self.parse_rule() {
                    rules.push(rule);
                }
            }
            self.skip_ws_and_comments();
        }

        rules
    }

    fn parse_rule(&mut self) -> Option<Value> {
        let rule_start = self.pos;
        let prelude = self.parse_selector_list()?;

        self.skip_ws_and_comments();
        let block = self.parse_block()?;

        Some(json!({
            "type": "Rule",
            "prelude": prelude,
            "block": block,
            "start": self.abs(rule_start),
            "end": block["end"]
        }))
    }

    fn parse_selector_list(&mut self) -> Option<Value> {
        let start = self.pos;
        let mut selectors = Vec::new();

        loop {
            self.skip_ws_and_comments();
            if self.pos >= self.source.len() || self.source[self.pos..].starts_with('{') {
                break;
            }

            let selector = self.parse_selector()?;
            selectors.push(selector);

            self.skip_ws_and_comments();
            if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b',' {
                self.pos += 1; // skip comma
            } else {
                break;
            }
        }

        if selectors.is_empty() {
            return None;
        }

        let end = selectors.last().map(|s| s["end"].as_u64().unwrap() as u32)
            .unwrap_or(self.abs(start));

        Some(json!({
            "type": "SelectorList",
            "start": self.abs(start),
            "end": end,
            "children": selectors
        }))
    }

    fn parse_selector(&mut self) -> Option<Value> {
        let start = self.pos;
        let mut children = Vec::new();
        let mut last_parse_end = self.pos; // track position after last selector (before whitespace skip)

        self.skip_ws_and_comments();

        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == b'{' || ch == b',' || ch == b'}' {
                break;
            }

            self.skip_whitespace();
            if self.pos >= self.source.len() {
                break;
            }
            let ch = self.source.as_bytes()[self.pos];
            if ch == b'{' || ch == b',' || ch == b'}' {
                break;
            }

            let simple = self.parse_simple_selector()?;
            last_parse_end = self.pos; // position after parsing (including parens)
            children.push(simple);
        }

        if children.is_empty() {
            return None;
        }

        let end = children.last().map(|s| s["end"].as_u64().unwrap() as u32)
            .unwrap_or(self.abs(start));
        let full_end = self.abs(last_parse_end);

        Some(json!({
            "type": "Selector",
            "start": self.abs(start),
            "_full_end": full_end,
            "end": end,
            "children": children
        }))
    }

    fn parse_simple_selector(&mut self) -> Option<Value> {
        let start = self.pos;
        let ch = self.source.as_bytes()[self.pos];

        match ch {
            b'.' => {
                // Class selector
                self.pos += 1;
                let name_start = self.pos;
                self.read_ident();
                let name = &self.source[name_start..self.pos];
                Some(json!({
                    "type": "ClassSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }))
            }
            b'#' => {
                // ID selector
                self.pos += 1;
                let name_start = self.pos;
                self.read_ident();
                let name = &self.source[name_start..self.pos];
                Some(json!({
                    "type": "IdSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }))
            }
            b'[' => {
                // Attribute selector
                self.pos += 1;
                let content_start = self.pos;
                let mut depth = 1;
                while self.pos < self.source.len() && depth > 0 {
                    match self.source.as_bytes()[self.pos] {
                        b'[' => depth += 1,
                        b']' => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 { self.pos += 1; }
                }
                let name = self.source[content_start..self.pos].trim();
                self.pos += 1; // skip ]
                Some(json!({
                    "type": "AttributeSelector",
                    "name": name,
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }))
            }
            b':' => {
                self.pos += 1;
                if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b':' {
                    // Pseudo-element
                    self.pos += 1;
                    let name_start = self.pos;
                    self.read_ident();
                    let name = &self.source[name_start..self.pos];
                    let name_end = self.pos;
                    // Check for args in parens
                    if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b'(' {
                        self.skip_parens();
                    }
                    // PseudoElementSelector end at name, but parser pos past parens
                    Some(json!({
                        "type": "PseudoElementSelector",
                        "name": name,
                        "start": self.abs(start),
                        "end": self.abs(name_end)
                    }))
                } else {
                    // Pseudo-class
                    let name_start = self.pos;
                    self.read_ident();
                    let name = &self.source[name_start..self.pos];
                    // Check for args in parens — parse as SelectorList for :global/:is/:where
                    if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b'(' {
                        let args_start = self.pos;
                        self.pos += 1; // skip (
                        // Parse inner content as selectors
                        let inner_start = self.pos;
                        let mut depth = 1;
                        while self.pos < self.source.len() && depth > 0 {
                            match self.source.as_bytes()[self.pos] {
                                b'(' => depth += 1,
                                b')' => depth -= 1,
                                _ => {}
                            }
                            if depth > 0 { self.pos += 1; }
                        }
                        let inner = &self.source[inner_start..self.pos];
                        let inner_offset = self.abs(inner_start);
                        // For :nth-* pseudo-classes, parse as Nth value
                        // Only parse as Nth if the content looks like An+B (not a plain selector)
                        let trimmed_inner = inner.trim();
                        let looks_like_nth = trimmed_inner.contains('n')
                            || trimmed_inner == "odd" || trimmed_inner == "even"
                            || trimmed_inner.chars().all(|c| c.is_ascii_digit() || c == '+' || c == '-' || c == ' ')
                            || trimmed_inner.contains(" of ");
                        let is_nth = name.starts_with("nth-") && looks_like_nth;
                        let args = if is_nth {
                            let trimmed = inner.trim();
                            // Check for "of <selector>" suffix — include "of " in the Nth value
                            let (nth_val, of_sel) = if let Some(of_pos) = trimmed.find(" of ") {
                                (&trimmed[..of_pos + 4], Some(&trimmed[of_pos + 4..]))
                            } else {
                                (trimmed, None)
                            };
                            let leading_ws = inner.len() - inner.trim_start().len();
                            let nth_start = inner_offset + leading_ws as u32;
                            let nth_val_end = nth_start + nth_val.len() as u32;
                            let nth_node = json!({
                                "type": "Nth",
                                "value": nth_val,
                                "start": nth_start,
                                "end": nth_val_end
                            });
                            // Build selectors array: Nth + optional selector entries
                            let mut selectors_arr = vec![nth_node];
                            let mut rel_end = nth_val_end;
                            if let Some(sel_str) = of_sel {
                                let sel_offset = inner_offset + (trimmed.len() - sel_str.len()) as u32;
                                let mut ip = CssParser::new(sel_str, sel_offset);
                                // Parse individual simple selectors (not full selector list)
                                while ip.pos < ip.source.len() {
                                    ip.skip_ws_and_comments();
                                    if ip.pos >= ip.source.len() { break; }
                                    if let Some(sel) = ip.parse_simple_selector() {
                                        rel_end = sel.get("end").and_then(|e| e.as_u64()).unwrap_or(0) as u32;
                                        selectors_arr.push(sel);
                                    } else { break; }
                                }
                            }
                            let full_end = inner_offset + inner.trim_end().len() as u32;
                            // Wrap in SelectorList → ComplexSelector → RelativeSelector
                            Some(json!({
                                "type": "SelectorList",
                                "start": nth_start,
                                "end": full_end,
                                "children": [{
                                    "type": "ComplexSelector",
                                    "start": nth_start,
                                    "end": full_end,
                                    "children": [{
                                        "type": "RelativeSelector",
                                        "combinator": null,
                                        "start": nth_start,
                                        "end": full_end,
                                        "selectors": selectors_arr
                                    }]
                                }]
                            }))
                        } else {
                            // Parse inner as selector list
                            let mut inner_parser = CssParser::new(inner, inner_offset);
                            inner_parser.parse_selector_list()
                        };
                        if self.pos < self.source.len() { self.pos += 1; } // skip )
                        let mut obj = json!({
                            "type": "PseudoClassSelector",
                            "name": name,
                            "start": self.abs(start),
                            "end": self.abs(self.pos)
                        });
                        if let Some(args_val) = args {
                            obj["args"] = args_val;
                        }
                        Some(obj)
                    } else {
                        Some(json!({
                            "type": "PseudoClassSelector",
                            "name": name,
                            "start": self.abs(start),
                            "end": self.abs(self.pos)
                        }))
                    }
                }
            }
            b'*' => {
                self.pos += 1;
                Some(json!({
                    "type": "TypeSelector",
                    "name": "*",
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }))
            }
            b'>' | b'+' | b'~' => {
                self.pos += 1;
                Some(json!({
                    "type": "Combinator",
                    "name": (ch as char).to_string(),
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }))
            }
            _ if ch.is_ascii_alphabetic() || ch == b'_' || ch == b'-' => {
                // Type selector
                let name_start = self.pos;
                self.read_ident();
                let name = &self.source[name_start..self.pos];
                // Check for namespace
                if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b'|' {
                    self.pos += 1;
                    self.read_ident();
                    let full_name = &self.source[name_start..self.pos];
                    Some(json!({
                        "type": "TypeSelector",
                        "name": full_name,
                        "start": self.abs(start),
                        "end": self.abs(self.pos)
                    }))
                } else {
                    Some(json!({
                        "type": "TypeSelector",
                        "name": name,
                        "start": self.abs(start),
                        "end": self.abs(self.pos)
                    }))
                }
            }
            b'%' => {
                // Percentage (for keyframes)
                self.pos += 1;
                let name_start = self.pos;
                while self.pos < self.source.len() && self.source.as_bytes()[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                Some(json!({
                    "type": "Percentage",
                    "value": &self.source[name_start..self.pos],
                    "start": self.abs(start),
                    "end": self.abs(self.pos)
                }))
            }
            _ => {
                // Unknown — skip one char to avoid infinite loop
                self.pos += 1;
                None
            }
        }
    }

    fn read_ident(&mut self) {
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_parens(&mut self) {
        if self.pos >= self.source.len() || self.source.as_bytes()[self.pos] != b'(' {
            return;
        }
        self.pos += 1;
        let mut depth = 1;
        while self.pos < self.source.len() && depth > 0 {
            match self.source.as_bytes()[self.pos] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            self.pos += 1;
        }
    }

    fn parse_block(&mut self) -> Option<Value> {
        if self.pos >= self.source.len() || self.source.as_bytes()[self.pos] != b'{' {
            return None;
        }

        let start = self.pos;
        self.pos += 1; // skip {
        self.skip_ws_and_comments();

        let mut children = Vec::new();

        while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != b'}' {
            // Check for nested rule
            if self.is_at_rule_start() {
                if let Some(rule) = self.parse_rule() {
                    children.push(rule);
                    self.skip_ws_and_comments();
                    continue;
                }
            }

            // Parse declaration
            if let Some(decl) = self.parse_declaration() {
                children.push(decl);
            }
            self.skip_ws_and_comments();
        }

        if self.pos < self.source.len() {
            self.pos += 1; // skip }
        }

        Some(json!({
            "type": "Block",
            "start": self.abs(start),
            "end": self.abs(self.pos),
            "children": children
        }))
    }

    fn is_at_rule_start(&self) -> bool {
        // Heuristic: if we see an identifier followed by {, it might be a nested rule
        // Skip this complexity for now
        false
    }

    fn parse_declaration(&mut self) -> Option<Value> {
        let start = self.pos;

        // Read property name
        let prop_start = self.pos;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == b':' || ch == b'}' || ch == b'{' {
                break;
            }
            self.pos += 1;
        }

        let property = self.source[prop_start..self.pos].trim();
        if property.is_empty() {
            return None;
        }

        if self.pos >= self.source.len() || self.source.as_bytes()[self.pos] != b':' {
            return None;
        }
        self.pos += 1; // skip :
        self.skip_whitespace();

        // Read value (skip quoted strings)
        let val_start = self.pos;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == b';' || ch == b'}' {
                break;
            }
            if ch == b'"' || ch == b'\'' {
                let q = ch;
                self.pos += 1;
                while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != q {
                    if self.source.as_bytes()[self.pos] == b'\\' { self.pos += 1; }
                    self.pos += 1;
                }
                if self.pos < self.source.len() { self.pos += 1; }
            } else {
                self.pos += 1;
            }
        }

        let value = self.source[val_start..self.pos].trim();

        // Find the actual end (before trailing whitespace and semicolon)
        let decl_end = self.pos;
        if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b';' {
            self.pos += 1; // skip ;
        }

        Some(json!({
            "type": "Declaration",
            "start": self.abs(start),
            "end": self.abs(decl_end),
            "property": property,
            "value": value
        }))
    }

    fn parse_atrule(&mut self) -> Option<Value> {
        let start = self.pos;
        self.pos += 1; // skip @

        let name_start = self.pos;
        self.read_ident();
        let name = self.source[name_start..self.pos].to_string();

        self.skip_whitespace();

        // Read prelude until { or ; (skip quoted strings)
        let prelude_start = self.pos;
        while self.pos < self.source.len() {
            let ch = self.source.as_bytes()[self.pos];
            if ch == b'{' || ch == b';' {
                break;
            }
            if ch == b'"' || ch == b'\'' {
                // Skip quoted string
                let quote = ch;
                self.pos += 1;
                while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != quote {
                    if self.source.as_bytes()[self.pos] == b'\\' { self.pos += 1; }
                    self.pos += 1;
                }
                if self.pos < self.source.len() { self.pos += 1; } // skip closing quote
            } else if ch == b'(' {
                // Skip parenthesized content (for url(...))
                self.pos += 1;
                let mut depth = 1;
                while self.pos < self.source.len() && depth > 0 {
                    match self.source.as_bytes()[self.pos] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        b'"' | b'\'' => {
                            let q = self.source.as_bytes()[self.pos];
                            self.pos += 1;
                            while self.pos < self.source.len() && self.source.as_bytes()[self.pos] != q {
                                if self.source.as_bytes()[self.pos] == b'\\' { self.pos += 1; }
                                self.pos += 1;
                            }
                        }
                        _ => {}
                    }
                    if depth > 0 { self.pos += 1; }
                }
                if self.pos < self.source.len() { self.pos += 1; }
            } else {
                self.pos += 1;
            }
        }
        let prelude = self.source[prelude_start..self.pos].trim().to_string();

        let block = if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b'{' {
            self.parse_block()
        } else {
            if self.pos < self.source.len() && self.source.as_bytes()[self.pos] == b';' {
                self.pos += 1;
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
}
