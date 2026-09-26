//! Require authored unsafe declarations to state their safety invariants.
//!
//! The pre-expansion pass reads parsed items before an attribute macro can
//! replace them. That includes the `unsafe extern "C++"` block inside a
//! `#[cxx::bridge]` module, which does not survive into HIR. Macro-generated
//! unsafe items introduced after this pass are outside its source boundary.
//! A syntactically present clause does not establish that its claim is true.

use clippy_utils::diagnostics::span_lint_and_help;
use quenchant_shape::shape::Maybe;
use rustc_ast::AssocItem;
use rustc_ast::AssocItemKind;
use rustc_ast::Attribute;
use rustc_ast::Item;
use rustc_ast::ItemKind;
use rustc_ast::Safety;
use rustc_lint::EarlyContext;
use rustc_lint::EarlyLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;

use crate::rustdoc::section_lines;
use crate::semantic::SectionHeading;

declare_lint! {
    /// ### What it does
    ///
    /// An authored `unsafe fn`, `unsafe trait`, `unsafe impl`, or `unsafe extern` block must have a `# Safety` section containing a nonempty `- unsafe invariants:` bullet. Unsafe associated functions carry their own section. Inside a `#[cxx::bridge]` module, the module's section covers its C++ extern blocks because cxx rejects doc attributes on those blocks.
    ///
    /// ### Why is this bad?
    ///
    /// An unsafe boundary without stated invariants gives callers and implementors no item-local account of the conditions they must preserve.
    ///
    /// ### Compiler stage and blind spots
    ///
    /// A pre-expansion AST pass sees C++ extern blocks before `#[cxx::bridge]` consumes their module. Unsafe items introduced only by later macro expansion are outside this source boundary. A module section can cover multiple bridge blocks, so this check cannot attribute individual invariant statements to each block. It validates a nonempty clause, not its truth or completeness.
    ///
    /// ### Example
    ///
    /// ```rust
    /// unsafe extern "C" { fn foreign(); }
    /// ```
    ///
    /// ```rust
    /// /// # Safety
    /// /// - unsafe invariants: callers ensure the foreign function preserves its ABI contract.
    /// unsafe extern "C" { fn foreign(); }
    /// ```
    pub UNSAFE_SAFETY_DOCUMENTATION,
    Deny,
    "authored unsafe declarations need a # Safety section with - unsafe invariants:"
}

impl_lint_pass!(SafetyDocumentation => [UNSAFE_SAFETY_DOCUMENTATION]);

/// Inspect unsafe declarations while their authored AST is still present.
#[derive(Default)]
#[repr(transparent)]
pub struct SafetyDocumentation
{
    /// Safety clauses for bridge modules currently being traversed.
    bridge_sections: Vec<SafetyClausePresent>,
}

impl EarlyLintPass for SafetyDocumentation
{
    /// Check free functions, traits, implementations, and foreign blocks.
    ///
    /// # Specification
    /// - ensures: reports a missing safety clause on each unsafe item and does
    ///   not report on safe items.
    /// - panics: none.
    fn check_item(
        &mut self,
        cx: &EarlyContext<'_>,
        item: &Item,
    )
    {
        if matches!(item.kind, ItemKind::Mod(..))
            && item
                .attrs
                .iter()
                .any(|attr| attr.path_matches(&[Symbol::intern("cxx"), Symbol::intern("bridge")]))
        {
            self.bridge_sections.push(safety_section(&item.attrs));
        }
        let unsafe_span = match item.kind {
            | ItemKind::Fn(ref function) => match function.sig.header.safety {
                | Safety::Unsafe(span) => Some(span),
                | Safety::Safe(_) | Safety::Default => None,
            },
            | ItemKind::Trait(ref trait_item) => match trait_item.safety {
                | Safety::Unsafe(span) => Some(span),
                | Safety::Safe(_) | Safety::Default => None,
            },
            | ItemKind::Impl(ref implementation) => {
                implementation
                    .of_trait
                    .as_ref()
                    .and_then(|header| match header.safety {
                        | Safety::Unsafe(span) => Some(span),
                        | Safety::Safe(_) | Safety::Default => None,
                    })
            },
            | ItemKind::ForeignMod(ref foreign) => match foreign.safety {
                | Safety::Unsafe(span) => Some(span),
                | Safety::Safe(_) | Safety::Default => None,
            },
            | _ => None,
        };
        if let Some(span) = unsafe_span
            && !(matches!(item.kind, ItemKind::ForeignMod(_))
                && self.bridge_sections.last().is_some_and(|section| section.0))
        {
            check_documentation(cx, &item.attrs, span);
        }
    }

    /// Close the enclosing bridge documentation scope after its nested items.
    ///
    /// # Specification
    /// - ensures: a bridge module's section never applies to later sibling
    ///   items.
    /// - panics: none.
    fn check_item_post(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &Item,
    )
    {
        if matches!(item.kind, ItemKind::Mod(..))
            && item
                .attrs
                .iter()
                .any(|attr| attr.path_matches(&[Symbol::intern("cxx"), Symbol::intern("bridge")]))
        {
            let _bridge = self.bridge_sections.pop();
        }
    }

    /// Check unsafe trait method declarations and defaults.
    ///
    /// # Specification
    /// - ensures: checks each unsafe trait function at its own documentation.
    /// - panics: none.
    fn check_trait_item(
        &mut self,
        cx: &EarlyContext<'_>,
        item: &AssocItem,
    )
    {
        if let AssocItemKind::Fn(ref function) = item.kind
            && let Safety::Unsafe(span) = function.sig.header.safety
        {
            check_documentation(cx, &item.attrs, span);
        }
    }

    /// Check unsafe implementation methods at their own documentation.
    ///
    /// # Specification
    /// - ensures: checks each unsafe implementation function independently of
    ///   its enclosing implementation's safety section.
    /// - panics: none.
    fn check_impl_item(
        &mut self,
        cx: &EarlyContext<'_>,
        item: &AssocItem,
    )
    {
        if let AssocItemKind::Fn(ref function) = item.kind
            && let Safety::Unsafe(span) = function.sig.header.safety
        {
            check_documentation(cx, &item.attrs, span);
        }
    }
}

/// Whether the relevant item's own section supplies an invariant bullet.
#[repr(transparent)]
struct SafetyClausePresent(bool);

/// A section is sufficient only when its own bullet states an invariant.
///
/// # Specification
/// - requires: `attrs` belong to the unsafe declaration marked by `span`.
/// - ensures: reports exactly when the declaration lacks a `# Safety` section
///   containing a nonempty `- unsafe invariants:` bullet before the next
///   heading.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — UI inputs distinguish missing sections, misplaced and
///   empty clauses, and nonempty clauses on ordinary and bridge declarations.
/// - witness: `tests::ui_safety`
fn check_documentation(
    cx: &EarlyContext<'_>,
    attrs: &[Attribute],
    span: Span,
)
{
    if safety_section(attrs).0 {
        return;
    }
    span_lint_and_help(
        cx,
        UNSAFE_SAFETY_DOCUMENTATION,
        span,
        "unsafe declaration lacks documented invariants",
        None,
        "add a # Safety section with a nonempty - unsafe invariants: bullet (on the bridge module for C++ extern blocks)",
    );
}

/// Read only bullets in this item's own safety section.
///
/// # Specification
/// - ensures: reports present exactly when a nonempty invariant bullet occurs
///   under the exact `# Safety` heading before the next heading.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — UI cases separate empty and misplaced bullets from valid
///   declarations and from the enclosing C++ bridge module's section.
/// - witness: `tests::ui_safety`
fn safety_section(attrs: &[Attribute]) -> SafetyClausePresent
{
    let mut lines = Vec::new();
    for attr in attrs {
        if let Some(doc) = attr.doc_str() {
            lines.extend(doc.as_str().lines().map(str::to_owned));
        }
    }
    let present = matches!(
        section_lines(&lines, SectionHeading::from("# Safety")),
        Maybe::Present(bullets) if bullets.iter().any(|bullet| {
            bullet.strip_prefix("- unsafe invariants:")
                .is_some_and(|value| !value.trim().is_empty())
        })
    );
    SafetyClausePresent(present)
}
