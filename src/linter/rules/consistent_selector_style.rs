//! `svelte/consistent-selector-style` — enforce consistent style selector usage
//! (e.g. prefer class selectors over element selectors).

use crate::linter::{LintContext, Rule};
use oxc::span::Span;

pub struct ConsistentSelectorStyle;

impl Rule for ConsistentSelectorStyle {
    fn name(&self) -> &'static str {
        "svelte/consistent-selector-style"
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        let style = match &ctx.ast.css {
            Some(s) => s,
            None => return,
        };

        let css = &style.content;
        let base = style.span.start as usize;

        // Heuristic: flag bare element selectors (a word at the start of a line
        // or after `{` / `,` that matches common HTML elements).
        const ELEMENT_SELECTORS: &[&str] = &[
            "div", "span", "p", "a", "ul", "ol", "li", "h1", "h2", "h3",
            "h4", "h5", "h6", "table", "tr", "td", "th", "section", "article",
            "header", "footer", "nav", "main", "aside", "form", "input",
            "button", "select", "textarea", "img", "label",
        ];

        for line in css.lines() {
            let trimmed = line.trim();
            for &el in ELEMENT_SELECTORS {
                if trimmed == el
                    || trimmed.starts_with(&format!("{} ", el))
                    || trimmed.starts_with(&format!("{},", el))
                    || trimmed.starts_with(&format!("{}{{", el))
                {
                    if let Some(pos) = css.find(line) {
                        let start = (base + pos) as u32;
                        let end = start + el.len() as u32;
                        ctx.diagnostic(
                            format!("Prefer class selectors over element selector `{}`.", el),
                            Span::new(start, end),
                        );
                    }
                    break;
                }
            }
        }
    }
}
