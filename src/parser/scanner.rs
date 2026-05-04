//! Shared scanner utilities for Svelte source.
//!
//! This is intentionally small: it does not build the final template AST, but
//! it owns the source-boundary logic that would otherwise be duplicated across
//! region extraction, template block parsing, and serialization helpers.

use oxc::span::Span;

#[derive(Debug, Clone)]
pub(crate) struct SvelteToken<'a> {
    pub(crate) kind: TokenKind<'a>,
    pub(crate) span: Span,
}

#[derive(Debug, Clone)]
pub(crate) enum TokenKind<'a> {
    Text(&'a str),
    HtmlComment(&'a str),
    StartTag {
        name: &'a str,
        attrs: &'a str,
        self_closing: bool,
    },
    EndTag {
        name: &'a str,
    },
    RawRegion {
        name: &'a str,
        attrs: &'a str,
        attrs_span: Span,
        content: &'a str,
        content_span: Span,
        closed: bool,
    },
    Mustache {
        expression: &'a str,
    },
    BlockStart {
        keyword: &'a str,
        expression: &'a str,
    },
    BlockContinuation {
        keyword: &'a str,
        expression: &'a str,
    },
    BlockEnd {
        keyword: &'a str,
    },
}

pub(crate) struct SvelteScanner<'a> {
    source: &'a str,
    pos: usize,
}

impl<'a> SvelteScanner<'a> {
    pub(crate) fn new(source: &'a str) -> Self {
        Self { source, pos: 0 }
    }

    fn read_tag_name(&self, pos: usize) -> usize {
        read_tag_name_end(self.source, pos)
    }

    fn read_svelte_keyword(&self, mut pos: usize) -> usize {
        while pos < self.source.len() {
            let ch = self.source.as_bytes()[pos];
            if ch.is_ascii_alphabetic() {
                pos += 1;
            } else {
                break;
            }
        }
        pos
    }
}

impl<'a> Iterator for SvelteScanner<'a> {
    type Item = SvelteToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.source.len() {
            return None;
        }

        let start = self.pos;

        if self.source[start..].starts_with("<!--") {
            let content_start = start + 4;
            let end = self.source[content_start..]
                .find("-->")
                .map(|idx| content_start + idx)
                .unwrap_or(self.source.len());
            self.pos = (end + 3).min(self.source.len());
            return Some(SvelteToken {
                kind: TokenKind::HtmlComment(&self.source[content_start..end]),
                span: Span::new(start as u32, self.pos as u32),
            });
        }

        if self.source.as_bytes()[start] == b'<' {
            if self.source[start..].starts_with("</") {
                let name_start = start + 2;
                let name_end = self.read_tag_name(name_start);
                let tag_end = find_tag_end(self.source, name_end).unwrap_or(self.source.len());
                self.pos = (tag_end + 1).min(self.source.len());
                return Some(SvelteToken {
                    kind: TokenKind::EndTag {
                        name: &self.source[name_start..name_end],
                    },
                    span: Span::new(start as u32, self.pos as u32),
                });
            }

            let name_start = start + 1;
            let name_end = self.read_tag_name(name_start);
            if name_start == name_end {
                self.pos += 1;
                return Some(SvelteToken {
                    kind: TokenKind::Text(&self.source[start..self.pos]),
                    span: Span::new(start as u32, self.pos as u32),
                });
            }

            let tag_end = find_tag_end(self.source, name_end).unwrap_or(self.source.len());
            let attrs = &self.source[name_end..tag_end];
            let self_closing = attrs.trim_end().ends_with('/');
            let name = &self.source[name_start..name_end];

            if !self_closing
                && (name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style"))
            {
                let content_start = (tag_end + 1).min(self.source.len());
                let (content_end, block_end, closed) =
                    find_close_tag(self.source, name, content_start)
                        .map(|(content_end, block_end)| (content_end, block_end, true))
                        .unwrap_or((self.source.len(), self.source.len(), false));
                self.pos = block_end;
                return Some(SvelteToken {
                    kind: TokenKind::RawRegion {
                        name,
                        attrs,
                        attrs_span: Span::new(name_end as u32, tag_end as u32),
                        content: &self.source[content_start..content_end],
                        content_span: Span::new(content_start as u32, content_end as u32),
                        closed,
                    },
                    span: Span::new(start as u32, block_end as u32),
                });
            }

            self.pos = (tag_end + 1).min(self.source.len());
            return Some(SvelteToken {
                kind: TokenKind::StartTag {
                    name,
                    attrs,
                    self_closing,
                },
                span: Span::new(start as u32, self.pos as u32),
            });
        }

        if self.source.as_bytes()[start] == b'{' {
            let expression_start;
            let kind = if self.source[start..].starts_with("{#")
                || self.source[start..].starts_with("{:")
                || self.source[start..].starts_with("{/")
                || self.source[start..].starts_with("{@")
            {
                let prefix_end = start + 2;
                let keyword_end = self.read_svelte_keyword(prefix_end);
                let keyword = &self.source[prefix_end..keyword_end];
                expression_start = keyword_end;
                let expression_end = find_expression_end(self.source, expression_start);
                let expression = &self.source[expression_start..expression_end];
                self.pos = (expression_end + 1).min(self.source.len());
                match self.source.as_bytes()[start + 1] {
                    b'#' | b'@' => TokenKind::BlockStart {
                        keyword,
                        expression,
                    },
                    b':' => TokenKind::BlockContinuation {
                        keyword,
                        expression,
                    },
                    b'/' => TokenKind::BlockEnd { keyword },
                    _ => unreachable!(),
                }
            } else {
                expression_start = start + 1;
                let expression_end = find_expression_end(self.source, expression_start);
                self.pos = (expression_end + 1).min(self.source.len());
                TokenKind::Mustache {
                    expression: &self.source[expression_start..expression_end],
                }
            };

            return Some(SvelteToken {
                kind,
                span: Span::new(start as u32, self.pos as u32),
            });
        }

        let mut end = start;
        while end < self.source.len() {
            let ch = self.source.as_bytes()[end];
            if ch == b'<' || ch == b'{' {
                break;
            }
            end += utf8_char_len(ch);
        }
        self.pos = end;
        Some(SvelteToken {
            kind: TokenKind::Text(&self.source[start..end]),
            span: Span::new(start as u32, end as u32),
        })
    }
}

pub(crate) fn find_tag_end(source: &str, mut pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut quote = None;

    while pos < source.len() {
        let ch = bytes[pos];
        if let Some(q) = quote {
            if ch == b'{' {
                let end = find_expression_end(source, pos + 1);
                pos = (end + 1).min(source.len());
                continue;
            }
            if ch == q {
                quote = None;
            }
            pos += 1;
            continue;
        }

        match ch {
            b'\'' | b'"' => {
                quote = Some(ch);
                pos += 1;
            }
            b'{' => {
                let end = find_expression_end(source, pos + 1);
                pos = (end + 1).min(source.len());
            }
            b'>' => return Some(pos),
            _ => pos += utf8_char_len(ch),
        }
    }

    None
}

pub(crate) fn find_expression_end(source: &str, mut pos: usize) -> usize {
    let start = pos;
    let bytes = source.as_bytes();
    let mut depth = 0i32;

    while pos < source.len() {
        match bytes[pos] {
            b'{' => {
                depth += 1;
                pos += 1;
            }
            b'}' => {
                if depth == 0 {
                    return pos;
                }
                depth -= 1;
                pos += 1;
            }
            b'\'' | b'"' | b'`' => pos = skip_string_literal(source, pos, bytes[pos]),
            b'/' if pos + 1 < source.len() => match bytes[pos + 1] {
                b'/' => pos = skip_line_comment(source, pos),
                b'*' => pos = skip_block_comment(source, pos),
                _ if slash_starts_regex(source, start, pos) => {
                    pos = skip_regex_literal(source, pos)
                }
                _ => pos += 1,
            },
            _ => pos += utf8_char_len(bytes[pos]),
        }
    }

    source.len()
}

pub(crate) fn find_top_level_spaced_word(source: &str, word: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut pos = 0;
    let mut depth = 0i32;

    while pos < source.len() {
        match bytes[pos] {
            b'\'' | b'"' | b'`' => {
                pos = skip_string_literal(source, pos, bytes[pos]);
                continue;
            }
            b'/' if pos + 1 < source.len() => match bytes[pos + 1] {
                b'/' => {
                    pos = skip_line_comment(source, pos);
                    continue;
                }
                b'*' => {
                    pos = skip_block_comment(source, pos);
                    continue;
                }
                _ if slash_starts_regex(source, 0, pos) => {
                    pos = skip_regex_literal(source, pos);
                    continue;
                }
                _ => {}
            },
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }

        if depth == 0
            && source[pos..].starts_with(word)
            && has_spaced_word_boundary(source, pos, word.len())
        {
            return Some(pos);
        }

        pos += utf8_char_len(bytes[pos]);
    }

    None
}

pub(crate) fn find_top_level_comma(source: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut pos = 0;
    let mut depth = 0i32;

    while pos < source.len() {
        match bytes[pos] {
            b'\'' | b'"' | b'`' => {
                pos = skip_string_literal(source, pos, bytes[pos]);
                continue;
            }
            b'/' if pos + 1 < source.len() => match bytes[pos + 1] {
                b'/' => {
                    pos = skip_line_comment(source, pos);
                    continue;
                }
                b'*' => {
                    pos = skip_block_comment(source, pos);
                    continue;
                }
                _ if slash_starts_regex(source, 0, pos) => {
                    pos = skip_regex_literal(source, pos);
                    continue;
                }
                _ => {}
            },
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => return Some(pos),
            _ => {}
        }
        pos += utf8_char_len(bytes[pos]);
    }

    None
}

pub(crate) fn find_trailing_top_level_parens(source: &str) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut pos = 0;
    let mut depth = 0i32;
    let mut open_at = None;
    let mut close_at = None;

    while pos < source.len() {
        match bytes[pos] {
            b'\'' | b'"' | b'`' => {
                pos = skip_string_literal(source, pos, bytes[pos]);
                continue;
            }
            b'/' if pos + 1 < source.len() => match bytes[pos + 1] {
                b'/' => {
                    pos = skip_line_comment(source, pos);
                    continue;
                }
                b'*' => {
                    pos = skip_block_comment(source, pos);
                    continue;
                }
                _ if slash_starts_regex(source, 0, pos) => {
                    pos = skip_regex_literal(source, pos);
                    continue;
                }
                _ => {}
            },
            b'(' if depth == 0 => {
                open_at = Some(pos);
                depth += 1;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' if depth == 1 => {
                close_at = Some(pos);
                depth -= 1;
            }
            b')' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        pos += utf8_char_len(bytes[pos]);
    }

    let (open, close) = (open_at?, close_at?);
    if source[close + 1..].trim().is_empty() {
        Some((open, close))
    } else {
        None
    }
}

pub(crate) fn is_svelte_keyword_boundary(source: &str, pos: usize) -> bool {
    source
        .as_bytes()
        .get(pos)
        .is_none_or(|ch| ch.is_ascii_whitespace() || *ch == b'}')
}

pub(crate) fn is_tag_name_boundary(source: &str, pos: usize) -> bool {
    source
        .as_bytes()
        .get(pos)
        .is_none_or(|ch| ch.is_ascii_whitespace() || *ch == b'>' || *ch == b'/')
}

pub(crate) fn read_tag_name_end(source: &str, mut pos: usize) -> usize {
    while pos < source.len() && !is_tag_name_boundary(source, pos) {
        pos += utf8_char_len(source.as_bytes()[pos]);
    }
    pos
}

fn find_close_tag(source: &str, tag_name: &str, mut search_from: usize) -> Option<(usize, usize)> {
    let prefix = format!("</{}", tag_name);
    while let Some(pos) = find_ascii_case_insensitive(&source[search_from..], &prefix) {
        let abs_pos = search_from + pos;
        if !is_tag_name_boundary(source, abs_pos + prefix.len()) {
            search_from = abs_pos + prefix.len();
            continue;
        }
        let mut end = abs_pos + prefix.len();
        while end < source.len() && source.as_bytes()[end].is_ascii_whitespace() {
            end += 1;
        }
        if end < source.len() && source.as_bytes()[end] == b'>' {
            return Some((abs_pos, end + 1));
        }
        search_from = abs_pos + prefix.len();
    }
    None
}

pub(crate) fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }

    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

pub(crate) fn starts_with_ascii_case_insensitive(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .get(..needle.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(needle.as_bytes()))
}

pub(crate) fn is_html_void_element(name: &str) -> bool {
    const VOID_ELEMENTS: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr",
    ];

    VOID_ELEMENTS
        .iter()
        .any(|void| name.eq_ignore_ascii_case(void))
}

fn has_spaced_word_boundary(source: &str, start: usize, word_len: usize) -> bool {
    let before_ok = start > 0 && source.as_bytes()[start - 1].is_ascii_whitespace();
    let after = start + word_len;
    let after_ok = after == source.len()
        || source
            .as_bytes()
            .get(after)
            .is_some_and(|ch| ch.is_ascii_whitespace());
    before_ok && after_ok
}

fn skip_string_literal(source: &str, mut pos: usize, quote: u8) -> usize {
    pos += 1;
    while pos < source.len() {
        let ch = source.as_bytes()[pos];
        if ch == b'\\' {
            pos = (pos + 2).min(source.len());
            continue;
        }
        if quote == b'`'
            && ch == b'$'
            && pos + 1 < source.len()
            && source.as_bytes()[pos + 1] == b'{'
        {
            let end = find_expression_end(source, pos + 2);
            pos = (end + 1).min(source.len());
            continue;
        }
        if ch == quote {
            return pos + 1;
        }
        pos += utf8_char_len(ch);
    }
    source.len()
}

fn skip_line_comment(source: &str, mut pos: usize) -> usize {
    pos += 2;
    while pos < source.len() && source.as_bytes()[pos] != b'\n' {
        pos += 1;
    }
    pos
}

fn skip_block_comment(source: &str, mut pos: usize) -> usize {
    pos += 2;
    while pos + 1 < source.len() {
        if source.as_bytes()[pos] == b'*' && source.as_bytes()[pos + 1] == b'/' {
            return pos + 2;
        }
        pos += 1;
    }
    source.len()
}

fn skip_regex_literal(source: &str, mut pos: usize) -> usize {
    pos += 1;
    let mut in_char_class = false;
    while pos < source.len() {
        let ch = source.as_bytes()[pos];
        if ch == b'\\' {
            pos = (pos + 2).min(source.len());
            continue;
        }
        if in_char_class {
            if ch == b']' {
                in_char_class = false;
            }
            pos += 1;
            continue;
        }
        match ch {
            b'[' => {
                in_char_class = true;
                pos += 1;
            }
            b'/' => {
                pos += 1;
                while pos < source.len() && source.as_bytes()[pos].is_ascii_alphabetic() {
                    pos += 1;
                }
                return pos;
            }
            b'\n' | b'\r' => return pos,
            _ => pos += utf8_char_len(ch),
        }
    }
    pos
}

fn slash_starts_regex(source: &str, expr_start: usize, pos: usize) -> bool {
    let bytes = source.as_bytes();
    let mut i = pos;
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

#[inline]
fn utf8_char_len(byte: u8) -> usize {
    if byte < 0xC0 {
        1
    } else if byte < 0xE0 {
        2
    } else if byte < 0xF0 {
        3
    } else {
        4
    }
}
