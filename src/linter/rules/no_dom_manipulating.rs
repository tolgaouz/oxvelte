//! `svelte/no-dom-manipulating` — disallow DOM manipulating.
//! ⭐ Recommended

use crate::linter::{LintContext, Rule};

const DOM_METHODS: &[&str] = &[
    ".appendChild(", ".removeChild(", ".insertBefore(", ".replaceChild(",
    ".remove()", ".setAttribute(", ".removeAttribute(", ".classList.",
    ".innerHTML", ".outerHTML", ".textContent", ".innerText",
    "document.createElement(", "document.createTextNode(",
    "document.getElementById(", "document.querySelector(",
    "document.querySelectorAll(",
];

pub struct NoDomManipulating;

impl Rule for NoDomManipulating {
    fn name(&self) -> &'static str {
        "svelte/no-dom-manipulating"
    }

    fn is_recommended(&self) -> bool {
        true
    }

    fn run<'a>(&self, ctx: &mut LintContext<'a>) {
        if let Some(script) = &ctx.ast.instance {
            let content = &script.content;
            let tag_start = script.span.start as usize;
            let source = ctx.source;

            for method in DOM_METHODS {
                let mut search_from = 0;
                while let Some(pos) = content[search_from..].find(method) {
                    let abs_content_pos = search_from + pos;
                    let tag_text = &source[tag_start..script.span.end as usize];
                    if let Some(gt) = tag_text.find('>') {
                        let source_pos = tag_start + gt + 1 + abs_content_pos;
                        ctx.diagnostic(
                            format!("Avoid direct DOM manipulation. Use Svelte's reactive declarations instead."),
                            oxc::span::Span::new(source_pos as u32, (source_pos + method.len()) as u32),
                        );
                    }
                    search_from = abs_content_pos + method.len();
                }
            }
        }
    }
}
