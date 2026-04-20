//! Typed CSS-selector AST for linter rules, built on the Servo `selectors`
//! crate. We parse each rule's selector text into a `SelectorList<OxSelector>`
//! and walk typed `Component` variants (Class, ID, LocalName, …).
//!
//! This is **not** a stylesheet parser — rule / at-rule / declaration splitting
//! lives in `src/parser/css.rs` and feeds per-rule selector strings here.
//!
//! Svelte's `:global(...)` pseudo-class has no builtin support; we recognize
//! it via `parse_non_ts_functional_pseudo_class` and carry the inner selector
//! list as an `OxPseudoClass::Global(...)` variant. `walk_components` descends
//! into it (respecting `check_global`), as well as `:is()`, `:where()`,
//! `:not()`, `:has()`.
//!
//! We never match selectors against a DOM — no matching code, no visitor
//! hooks. `NonTSPseudoClass::is_active_or_hover` and friends return `false`.

use cssparser::{CowRcStr, Parser as CssParser, ParserInput, SourceLocation, ToCss};
use selectors::parser::{
    Component as SelComponent, NonTSPseudoClass, ParseRelative, Parser as SelParser,
    PseudoElement, Selector, SelectorImpl, SelectorList, SelectorParseErrorKind,
};

/// Tiny string wrapper satisfying `selectors`' identifier / atom bounds.
///
/// `selectors` requires `PrecomputedHash` on `Identifier` / `LocalName` /
/// `NamespaceUrl` so it can hash-bucket selectors at match time. We never
/// actually match, but we still have to provide *something*. Storing the hash
/// alongside the string lets us avoid re-hashing the same atom repeatedly.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct OxAtom {
    value: Box<str>,
    hash: u32,
}

impl OxAtom {
    fn new(s: &str) -> Self {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = rustc_hash::FxBuildHasher::default().build_hasher();
        hasher.write(s.as_bytes());
        OxAtom { value: s.into(), hash: hasher.finish() as u32 }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl<'a> From<&'a str> for OxAtom {
    fn from(s: &'a str) -> Self {
        OxAtom::new(s)
    }
}

impl std::borrow::Borrow<str> for OxAtom {
    fn borrow(&self) -> &str {
        &self.value
    }
}

impl precomputed_hash::PrecomputedHash for OxAtom {
    fn precomputed_hash(&self) -> u32 {
        self.hash
    }
}

impl ToCss for OxAtom {
    fn to_css<W: std::fmt::Write>(&self, dest: &mut W) -> std::fmt::Result {
        cssparser::serialize_identifier(&self.value, dest)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OxPseudoClass {
    /// `:global(<selector-list>)` — Svelte-specific.
    Global(SelectorList<OxSelector>),
    /// Any other `:foo` or `:foo(...)` whose body we didn't need to inspect.
    Other(OxAtom),
}

impl ToCss for OxPseudoClass {
    fn to_css<W: std::fmt::Write>(&self, dest: &mut W) -> std::fmt::Result {
        match self {
            OxPseudoClass::Global(inner) => {
                dest.write_str(":global(")?;
                inner.to_css(dest)?;
                dest.write_str(")")
            }
            OxPseudoClass::Other(name) => {
                dest.write_char(':')?;
                name.to_css(dest)
            }
        }
    }
}

impl NonTSPseudoClass for OxPseudoClass {
    type Impl = OxSelector;
    fn is_active_or_hover(&self) -> bool {
        false
    }
    fn is_user_action_state(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OxPseudoElement(pub OxAtom);

impl ToCss for OxPseudoElement {
    fn to_css<W: std::fmt::Write>(&self, dest: &mut W) -> std::fmt::Result {
        dest.write_str("::")?;
        self.0.to_css(dest)
    }
}

impl PseudoElement for OxPseudoElement {
    type Impl = OxSelector;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OxSelector;

impl SelectorImpl for OxSelector {
    type ExtraMatchingData<'a> = ();
    type AttrValue = OxAtom;
    type Identifier = OxAtom;
    type LocalName = OxAtom;
    type NamespaceUrl = OxAtom;
    type NamespacePrefix = OxAtom;
    type BorrowedNamespaceUrl = str;
    type BorrowedLocalName = str;
    type NonTSPseudoClass = OxPseudoClass;
    type PseudoElement = OxPseudoElement;
}

pub struct OxSelectorParser;

impl<'i> SelParser<'i> for OxSelectorParser {
    type Impl = OxSelector;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_is_and_where(&self) -> bool {
        true
    }
    fn parse_has(&self) -> bool {
        true
    }
    fn parse_part(&self) -> bool {
        true
    }
    fn parse_slotted(&self) -> bool {
        true
    }
    fn parse_host(&self) -> bool {
        true
    }
    fn parse_nth_child_of(&self) -> bool {
        true
    }

    fn parse_non_ts_pseudo_class(
        &self,
        _location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<OxPseudoClass, cssparser::ParseError<'i, Self::Error>> {
        Ok(OxPseudoClass::Other(OxAtom::new(&name)))
    }

    fn parse_non_ts_functional_pseudo_class<'t>(
        &self,
        name: CowRcStr<'i>,
        parser: &mut CssParser<'i, 't>,
        _after_part: bool,
    ) -> Result<OxPseudoClass, cssparser::ParseError<'i, Self::Error>> {
        if name.eq_ignore_ascii_case("global") {
            let list = SelectorList::parse(self, parser, ParseRelative::No)?;
            Ok(OxPseudoClass::Global(list))
        } else {
            // Skip the argument tokens so parsing doesn't explode on
            // unknown functional pseudo-classes we don't special-case.
            let _ = parser.parse_entirely(|p| {
                while !p.is_exhausted() {
                    p.next()?;
                }
                Ok::<_, cssparser::ParseError<'i, Self::Error>>(())
            });
            Ok(OxPseudoClass::Other(OxAtom::new(&name)))
        }
    }

    fn parse_pseudo_element(
        &self,
        _location: SourceLocation,
        name: CowRcStr<'i>,
    ) -> Result<OxPseudoElement, cssparser::ParseError<'i, Self::Error>> {
        Ok(OxPseudoElement(OxAtom::new(&name)))
    }

    fn parse_functional_pseudo_element<'t>(
        &self,
        name: CowRcStr<'i>,
        arguments: &mut CssParser<'i, 't>,
    ) -> Result<OxPseudoElement, cssparser::ParseError<'i, Self::Error>> {
        let _ = arguments.parse_entirely(|p| {
            while !p.is_exhausted() {
                p.next()?;
            }
            Ok::<_, cssparser::ParseError<'i, Self::Error>>(())
        });
        Ok(OxPseudoElement(OxAtom::new(&name)))
    }
}

/// Parse a comma-separated selector list. Returns `None` if parsing fails
/// anywhere — the linter rules are best-effort and shouldn't bail loudly on
/// unparseable CSS (vendor behaves the same way: unparseable rules are just
/// skipped).
pub fn parse_selector_list(text: &str) -> Option<SelectorList<OxSelector>> {
    let mut input = ParserInput::new(text);
    let mut parser = CssParser::new(&mut input);
    SelectorList::parse(&OxSelectorParser, &mut parser, ParseRelative::No).ok()
}

/// Iterate every `Component` across every `Selector` in a list, descending
/// into `:is()` / `:where()` / `:not()` / `:has()` and into `:global(...)`
/// when `visit_global` is true. The callback receives `(component, in_global)`
/// so rules can distinguish globally-scoped classes/ids from local ones.
pub fn walk_components<F>(
    list: &SelectorList<OxSelector>,
    visit_global: bool,
    f: &mut F,
) where
    F: FnMut(&SelComponent<OxSelector>, bool),
{
    for selector in list.slice() {
        walk_selector(selector, visit_global, false, f);
    }
}

fn walk_selector<F>(
    selector: &Selector<OxSelector>,
    visit_global: bool,
    in_global: bool,
    f: &mut F,
) where
    F: FnMut(&SelComponent<OxSelector>, bool),
{
    for component in selector.iter_raw_match_order() {
        visit_component(component, visit_global, in_global, f);
    }
}

fn visit_component<F>(
    component: &SelComponent<OxSelector>,
    visit_global: bool,
    in_global: bool,
    f: &mut F,
) where
    F: FnMut(&SelComponent<OxSelector>, bool),
{
    f(component, in_global);
    match component {
        SelComponent::NonTSPseudoClass(OxPseudoClass::Global(inner)) => {
            if visit_global {
                for selector in inner.slice() {
                    walk_selector(selector, visit_global, true, f);
                }
            }
        }
        SelComponent::Is(inner) | SelComponent::Where(inner) | SelComponent::Negation(inner) => {
            for selector in inner.slice() {
                walk_selector(selector, visit_global, in_global, f);
            }
        }
        SelComponent::Has(relatives) => {
            for rel in relatives.iter() {
                walk_selector(&rel.selector, visit_global, in_global, f);
            }
        }
        SelComponent::Slotted(inner) => walk_selector(inner, visit_global, in_global, f),
        SelComponent::Host(Some(inner)) => walk_selector(inner, visit_global, in_global, f),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_class_id_tag() {
        let list = parse_selector_list(".foo #bar p").expect("should parse");
        let mut classes = Vec::new();
        let mut ids = Vec::new();
        let mut tags = Vec::new();
        walk_components(&list, true, &mut |c, _| match c {
            SelComponent::Class(a) => classes.push(a.as_str().to_string()),
            SelComponent::ID(a) => ids.push(a.as_str().to_string()),
            SelComponent::LocalName(l) => tags.push(l.name.as_str().to_string()),
            _ => {}
        });
        assert_eq!(classes, vec!["foo"]);
        assert_eq!(ids, vec!["bar"]);
        assert_eq!(tags, vec!["p"]);
    }

    #[test]
    fn handles_global_pseudo() {
        let list = parse_selector_list(":global(.foo) .bar").expect("should parse");

        let mut local = Vec::new();
        walk_components(&list, false, &mut |c, _| {
            if let SelComponent::Class(a) = c {
                local.push(a.as_str().to_string());
            }
        });
        assert_eq!(local, vec!["bar"], "with visit_global=false, :global body is hidden");

        let mut all = Vec::new();
        walk_components(&list, true, &mut |c, in_global| {
            if let SelComponent::Class(a) = c {
                all.push((a.as_str().to_string(), in_global));
            }
        });
        assert!(all.contains(&("foo".to_string(), true)));
        assert!(all.contains(&("bar".to_string(), false)));
    }

    #[test]
    fn descends_into_is_where_not() {
        let list = parse_selector_list(":is(.a, .b) :not(.c) :where(.d)").expect("should parse");
        let mut classes = Vec::new();
        walk_components(&list, true, &mut |c, _| {
            if let SelComponent::Class(a) = c {
                classes.push(a.as_str().to_string());
            }
        });
        classes.sort();
        assert_eq!(classes, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn survives_bad_input() {
        assert!(parse_selector_list("").is_none());
        assert!(parse_selector_list("{{{").is_none());
    }
}
