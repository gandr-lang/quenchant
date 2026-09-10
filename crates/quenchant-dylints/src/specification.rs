//! Authored specification presence without pretending to decide satisfaction.
//!
//! The gate requires an item's specification to be stated before its body is
//! credited. It recognizes the section and rejects contradictory uses of the
//! sole-body `trivial.` marker. It does not validate arbitrary predicate
//! meaning, establish completeness, or infer an implementation's intended
//! behavior.
//!
//! Free functions, methods, required trait declarations, foreign declarations,
//! and locally generated items can have author-owned documentation. Derived
//! items, foreign manufactured declarations, harness entry points, and test
//! functions follow their distinct authorship or test-syntax boundaries.
//!
//! A name span alone is insufficient: a generated client method can reuse the
//! author's identifier while its declaration remains foreign. Both provenance
//! questions therefore participate in the decision. If a consumer makes those
//! generated siblings documentation-owned, this rule needs an explicit revised
//! boundary rather than an accidental blanket exemption.
//!
//! Presence is one source-shape check. Adequacy grammar and runnable witness
//! resolution are separate stages, and a failure in generated checking still
//! needs classification regardless of who manufactured its declaration.

use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::in_automatically_derived;
use clippy_utils::is_in_test_function;
use quenchant_shape::shape::Maybe;
use rustc_hir::ForeignItem;
use rustc_hir::ForeignItemKind;
use rustc_hir::ImplItem;
use rustc_hir::ImplItemKind;
use rustc_hir::Item;
use rustc_hir::ItemKind;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

use crate::rustdoc::indented_rustdoc_lines;
use crate::rustdoc::section_body;
use crate::semantic::AuthoredItem;
use crate::semantic::DiagnosticText;
use crate::semantic::MarkerBesideClause;
use crate::semantic::MarkerWrittenAsBullet;
use crate::semantic::NameSpanCarriesIdentifier;
use crate::semantic::RustdocLine;
use crate::semantic::SectionHeading;

declare_lint! {
    /// ### What it does
    ///
    /// Authored functions and methods require a specification section. A trivial marker must be the section's only content and must not be a bullet.
    ///
    /// `trivial.` is the canonical body, including the punctuation required by the prose lint. The reader also accepts `trivial`; both spellings exclude bullet syntax and accompanying clauses.
    ///
    /// ### Why is this bad?
    ///
    /// The authored specification precedes the body and exposes its obligations to review. A missing section leaves those obligations unavailable. The trivial marker instead makes an explicit, reviewable claim that no further statement is needed; combining it with clauses contradicts that claim, while a bulleted marker is not the required declaration.
    ///
    /// ### The exception
    ///
    /// Three item classes are outside the rule. `#[test]` functions state their
    /// subject in the test name and their specification in the assertions.
    /// `#[automatically_derived]` items are a derive expansion, whose rustdoc
    /// the author of the deriving type cannot write. Closures are not items and
    /// carry no rustdoc at all, so a closure inside a documented function is
    /// covered by that function's block.
    ///
    /// Outside the explicit attribute exemptions, recognized authorship requires both an authored identifier and a crate-owned declaration signature. Foreign-generated functions and test entry points are exempt, including a manufactured method that borrows an authored name. Crate-local `macro_rules!` expansions remain covered: their author can place the specification in the template.
    ///
    /// Revisit declaration-based exemption if generated siblings acquire independently authored documentation obligations. That case requires narrower macro-source admission.
    ///
    /// ### Blind spots
    ///
    /// Heading presence and trivial-marker shape are the implemented checks. Empty bodies, misspelled or reordered clauses, and prose describing the wrong item remain outside this reader's validation.
    ///
    /// Name recognition compares source text with the item's identifier. Manufactured `__anodized_*` siblings and unavailable source text are exempt under that heuristic; the original declaration whose identifier matches the same span remains covered.
    ///
    /// Declaration recognition uses the signature rather than the attribute-bearing whole-item span, because rustc classifies attribute macros as external. A macro rebuilding authored signature tokens would also lose recognized authorship under this rule.
    ///
    /// ### Example
    ///
    /// ```rust
    /// /// Convert a zero-based coordinate into a one-based coordinate.
    /// fn one_based(value: ZeroBased) -> OneBased { value.successor() }
    /// ```
    ///
    /// An authored specification:
    ///
    /// ```rust
    /// /// Convert a zero-based coordinate into a one-based coordinate.
    /// ///
    /// /// # Specification
    /// /// - requires: `value` is a zero-based coordinate from an external protocol.
    /// /// - ensures: returns the successor of `value`.
    /// /// - panics: none.
    /// fn one_based(value: ZeroBased) -> OneBased { value.successor() }
    /// ```
    ///
    /// An explicit claim that no further statement is needed:
    ///
    /// ```rust
    /// /// Return the number of nodes in the arena.
    /// ///
    /// /// # Specification
    /// /// trivial.
    /// fn len(&self) -> NodeCount { self.nodes.len().into() }
    /// ```
    pub SPECIFICATION_PRESENT,
    Deny,
    "every authored function and method must carry a # Specification block"
}

impl_lint_pass!(WorkflowSpecification => [SPECIFICATION_PRESENT]);

/// Compiler-side enforcement of specification presence and trivial-marker
/// shape.
pub struct WorkflowSpecification;

impl<'tcx> LateLintPass<'tcx> for WorkflowSpecification
{
    /// A free function's authored name anchors its specification obligation.
    ///
    /// # Specification
    /// - ensures: checks a function item and ignores every other item kind,
    ///   reporting at the function's name where it has one.
    /// - panics: none.
    fn check_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx Item<'tcx>,
    )
    {
        let ItemKind::Fn { sig, .. } = item.kind
        else {
            return;
        };
        let span = item.kind.ident().map_or(item.span, |ident| ident.span);
        check_presence(cx, item.owner_id.def_id, span, sig.span);
    }

    /// Implementation methods retain their own specification obligations.
    ///
    /// # Specification
    /// - ensures: checks a method and ignores every other associated item kind.
    /// - panics: none.
    fn check_impl_item(
        &mut self,
        cx: &LateContext<'tcx>,
        impl_item: &'tcx ImplItem<'tcx>,
    )
    {
        let ImplItemKind::Fn(signature, _) = impl_item.kind
        else {
            return;
        };
        check_presence(
            cx,
            impl_item.owner_id.def_id,
            impl_item.ident.span,
            signature.span,
        );
    }

    /// Trait declarations and defaults are inspected at their own names.
    ///
    /// # Specification
    /// - ensures: checks a method declaration, with or without a body, and
    ///   ignores every other associated item kind.
    /// - panics: none.
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        trait_item: &'tcx TraitItem<'tcx>,
    )
    {
        let TraitItemKind::Fn(signature, _) = trait_item.kind
        else {
            return;
        };
        check_presence(
            cx,
            trait_item.owner_id.def_id,
            trait_item.ident.span,
            signature.span,
        );
    }

    /// Foreign function declarations remain specification-bearing authored
    /// interfaces.
    ///
    /// # Specification
    /// - ensures: checks a foreign function and ignores every other foreign
    ///   item kind.
    /// - panics: none.
    fn check_foreign_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx ForeignItem<'tcx>,
    )
    {
        let ForeignItemKind::Fn(signature, ..) = item.kind
        else {
            return;
        };
        check_presence(cx, item.owner_id.def_id, item.ident.span, signature.span);
    }
}

/// Missing or contradictory specification declarations.
enum SpecificationDefect
{
    /// The item's documentation supplies no exact specification heading.
    BlockAbsent,
    /// Another nonblank line accompanies the trivial marker.
    MarkerNotAlone,
    /// Bullet syntax replaces the required bare marker.
    MarkerAsBullet,
}

impl SpecificationDefect
{
    /// The defect selects a diagnostic describing the rejected declaration
    /// shape.
    ///
    /// # Specification
    /// trivial.
    fn message(&self) -> DiagnosticText<'static>
    {
        DiagnosticText(match *self {
            | Self::BlockAbsent => {
                "this function carries no `# Specification` rustdoc block; the specification is \
                 authored before the body it governs, so an absent block records that nothing was \
                 decided about what the body owes"
            },
            | Self::MarkerNotAlone => {
                "`# Specification` writes `trivial` beside another line; the marker claims there \
                 is nothing to specify, so a clause written next to it contradicts the claim"
            },
            | Self::MarkerAsBullet => {
                "`# Specification` writes `trivial` as a bullet; the marker is a claim about the \
                 whole block rather than one clause of it, so the body carries the word alone"
            },
        })
    }
}

/// Exact heading used to locate the item's authored specification.
const HEADING: &str = "# Specification";

/// Marker vocabulary for an explicit claim of trivial specification.
///
/// The accepted bare forms are `trivial.` and `trivial`; the punctuated form
/// also satisfies the prose lint. Neither permits accompanying clauses or
/// bullet syntax. This constant owns the shared marker spelling.
const TRIVIAL_MARKER: &str = "trivial";

/// Denial help exposes the admitted specification declaration forms.
const ACCEPTED_SHAPES: &str = concat!(
    "write the block as clauses in the fixed order — `- requires:`, `- ensures:`, `- provides:`, ",
    "`- fails:`, `- panics:`, `- intension:` — or, where the item has nothing to state, as the ",
    "canonical body `trivial.` with the terminal period, which the rule also accepts without it, ",
    "and in either case no bullet marker and no clause beside it",
);

/// Authorship and section shape determine whether this function receives a
/// presence diagnostic.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local function, method, or foreign
///   function declaration, `span` is the item's own name span, and
///   `declaration` is its own signature span.
/// - ensures: reports nothing for an item outside the rule — a `#[test]`
///   function, an `#[automatically_derived]` item, or an item whose name is not
///   this crate's own syntax — and otherwise reports
///   [`SpecificationDefect::BlockAbsent`] when no `# Specification` heading is
///   present, [`SpecificationDefect::MarkerNotAlone`] when the section holds
///   the trivial marker beside another line, and
///   [`SpecificationDefect::MarkerAsBullet`] when it writes the marker as a
///   bullet.
/// - provides: the denial [`SPECIFICATION_PRESENT`] reports.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates each firing shape and each
///   exclusion one at a time: an undocumented function, a documented function
///   with no block, the marker beside a clause, the marker as a bullet, a
///   clause block, a marker-only block, a `#[test]` function, a derive
///   expansion, a closure, and every declaration form the rule decides.
/// - witness: `tests::ui`
/// - witness: `tests::ui_specifications`
fn check_presence(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    span: Span,
    declaration: Span,
)
{
    if !authored(cx, def_id, declaration).0 {
        return;
    }
    let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
    if is_in_test_function(cx.tcx, hir_id) || in_automatically_derived(cx.tcx, hir_id) {
        return;
    }
    let lines = indented_rustdoc_lines(cx, def_id);
    let defect = match section_body(&lines, SectionHeading::from(HEADING)) {
        | Maybe::Absent(_) => SpecificationDefect::BlockAbsent,
        | Maybe::Present(body) => {
            if marker_beside_clause(&body).0 {
                SpecificationDefect::MarkerNotAlone
            }
            else if marker_written_as_bullet(&body).0 {
                SpecificationDefect::MarkerAsBullet
            }
            else {
                return;
            }
        },
    };
    span_lint_and_help(
        cx,
        SPECIFICATION_PRESENT,
        span,
        defect.message().0,
        None,
        ACCEPTED_SHAPES,
    );
}

/// Name and signature provenance jointly determine recognized declaration
/// ownership.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local item, and `declaration` is
///   that item's own signature span — its `fn` keyword through its return type,
///   carrying neither its attributes nor its body.
/// - ensures: answers affirmatively exactly when the declaration span is one
///   rustc does not treat as belonging to an external macro, and the item has a
///   name span that is not in such a macro either and whose source text is the
///   item's own identifier. Two spans are asked because a macro from another
///   crate can manufacture either half alone. The declaration is the right span
///   for the item: rustc counts every attribute macro as external and the whole
///   item span carries the attribute, so an item under `#[spec]` would read as
///   foreign there, while the signature such an attribute re-emits keeps the
///   author's context and a signature the macro wrote itself does not. The name
///   is asked separately because a macro can pass the author's own identifier
///   through into a declaration of its own, which the name span cannot
///   distinguish. A crate-local `macro_rules!` is not external, so its
///   expansions stay under the rule and are reported at the macro's own text.
/// - provides: the foreign-expansion exemption, the one exclusion the rule
///   derives rather than reads off an attribute.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the matrices separate a crate-local `macro_rules!`
///   expansion (reported), the test harness's generated entry point (exempt), a
///   real `#[spec]` expansion whose function is still reported at its name, the
///   sibling declaration a `#[spec]` trait definition synthesizes under the
///   author's name span (exempt), and the method a foreign attribute macro
///   generates under the author's own identifier (exempt) beside the authored
///   method of the same expansion and name (reported). `cargo-mutants` offers
///   one mutant here, the `Default::default()` return, and it is unviable:
///   [`AuthoredItem`] has no `Default`. The hand mutants on the declaration arm
///   are its inverted test and its affirmative answer, and the generated
///   sibling row kills both — inverting it also takes every authored item out
///   of the rule and empties both matrices.
/// - witness: `tests::ui`
/// - witness: `tests::ui_specifications`
fn authored(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    declaration: Span,
) -> AuthoredItem
{
    if declaration.in_external_macro(cx.tcx.sess.source_map()) {
        return AuthoredItem(false);
    }
    let Some(name) = cx.tcx.def_ident_span(def_id)
    else {
        return AuthoredItem(false);
    };
    if name.in_external_macro(cx.tcx.sess.source_map()) {
        return AuthoredItem(false);
    }
    AuthoredItem(name_span_carries_identifier(cx, def_id, name).0)
}

/// Source text must identify the declared item rather than a manufactured
/// sibling.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local item and `span` is that item's
///   name span.
/// - ensures: answers affirmatively exactly when the text written at `span` is
///   the item's identifier, tolerating the `r#` of a raw one, and negatively
///   when the item has no identifier or the compiler has no source text for the
///   span. An authored name is the token the source carries at that span; a
///   name a macro built out of another item's identifier keeps that
///   identifier's span, so the two disagree exactly where the name was
///   manufactured.
/// - provides: the manufactured-name half of the foreign-expansion exemption.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the matrices separate the sibling declaration a specified
///   trait definition synthesizes under the author's name span (exempt) from
///   the author's own declaration at that span (reported), and pin the `r#`
///   strip on an authored `fn r#type()`, whose written text and identifier
///   differ by exactly that prefix: without the strip the presence rule exempts
///   that function and the matrix loses a diagnostic. The two absence arms are
///   equivalent (semantic) survivors under `docs/agents/testing-contracts.md`
///   §survivor-taxonomy — the sole caller resolves the name span first, rustc
///   reads that span and the item name off one ident, and a crate-local name
///   span outside an external macro resolves in this crate's own source map, so
///   no valid input reaches either arm and no observation separates its answer
///   from an affirmative one. Both guards stay because the queries are fallible
///   and the item specifications no panic.
/// - witness: `tests::ui`
/// - witness: `tests::ui_specifications`
fn name_span_carries_identifier(
    cx: &LateContext<'_>,
    def_id: LocalDefId,
    span: Span,
) -> NameSpanCarriesIdentifier
{
    let Some(identifier) = cx.tcx.opt_item_name(def_id.to_def_id())
    else {
        return NameSpanCarriesIdentifier(false);
    };
    let Ok(written) = cx.tcx.sess.source_map().span_to_snippet(span)
    else {
        return NameSpanCarriesIdentifier(false);
    };
    let written = written.trim();
    NameSpanCarriesIdentifier(written.strip_prefix("r#").unwrap_or(written) == identifier.as_str())
}

/// A nonblank line's relationship to the admitted trivial-marker forms.
#[derive(Clone, Copy)]
enum MarkerSpelling
{
    /// The bare marker uses an accepted punctuated or unpunctuated spelling.
    Canonical,
    /// A bullet marker makes the declaration malformed.
    Bulleted,
    /// Clauses and ordinary prose are distinct from the trivial marker.
    NotTheMarker,
}

/// Marker recognition separates bare declarations from bullets and other
/// content.
///
/// # Specification
/// - requires: `line` is one nonblank line of a `# Specification` body.
/// - ensures: reads the word alone as [`MarkerSpelling::Canonical`] with or
///   without the terminal period, the same word behind a `-` bullet as
///   [`MarkerSpelling::Bulleted`], and every other line as
///   [`MarkerSpelling::NotTheMarker`]. The marker is the whole line, so prose
///   merely opening with the spelling is not it.
/// - provides: the marker recognition both denials below share.
/// - panics: none.
fn marker_spelling(line: RustdocLine<'_>) -> MarkerSpelling
{
    let text = line.0.trim();
    let (bulleted, word) = match text.strip_prefix('-') {
        | Some(rest) => (true, rest.trim()),
        | None => (false, text),
    };
    if word.strip_suffix('.').unwrap_or(word) != TRIVIAL_MARKER {
        return MarkerSpelling::NotTheMarker;
    }
    if bulleted {
        MarkerSpelling::Bulleted
    }
    else {
        MarkerSpelling::Canonical
    }
}

/// A marker is contradictory when another nonblank line shares its section.
///
/// # Specification
/// - requires: `body` is the unfolded body of a `# Specification` section, as
///   [`section_body`] returns it.
/// - ensures: answers affirmatively exactly when one nonblank line writes the
///   marker — in any spelling [`marker_spelling`] recognizes — and some other
///   nonblank line does not. The marker alone and clauses without a marker both
///   answer negatively, and so does a section with no content at all: presence
///   is what this rule decides.
/// - provides: the second denial [`SPECIFICATION_PRESENT`] reports.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate the marker alone, the marker with its
///   terminal period, the marker above a clause, the marker below a clause,
///   clauses with no marker, an empty section, a blank line beside the marker,
///   and a line that merely opens with the marker's spelling.
/// - witness: `specification::tests::the_bare_word_is_the_whole_body`
/// - witness: `specification::tests::the_marker_carries_the_terminal_period`
/// - witness: `specification::tests::the_marker_beside_a_clause_is_refused`
/// - witness: `specification::tests::clauses_without_the_marker_are_accepted`
fn marker_beside_clause(body: &[String]) -> MarkerBesideClause
{
    let mut marker = false;
    let mut clause = false;
    for line in body {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match marker_spelling(RustdocLine::from(trimmed)) {
            | MarkerSpelling::Canonical | MarkerSpelling::Bulleted => marker = true,
            | MarkerSpelling::NotTheMarker => clause = true,
        }
    }
    MarkerBesideClause(marker && clause)
}

/// Bullet-form markers remain invalid even when their section contains nothing
/// else.
///
/// # Specification
/// - requires: `body` is the unfolded body of a `# Specification` section, as
///   [`section_body`] returns it.
/// - ensures: answers affirmatively exactly when some nonblank line writes the
///   marker behind a `-` bullet. The canonical body answers negatively, and so
///   does an ordinary clause: the bullet is read as the marker only where the
///   word is the bullet's whole value.
/// - provides: the third denial [`SPECIFICATION_PRESENT`] reports.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate the bulleted marker, the bulleted
///   marker with its period, an ordinary clause list, and the canonical body.
/// - witness: `specification::tests::the_bulleted_marker_is_refused`
/// - witness: `specification::tests::clauses_without_the_marker_are_accepted`
fn marker_written_as_bullet(body: &[String]) -> MarkerWrittenAsBullet
{
    MarkerWrittenAsBullet(body.iter().any(|line| {
        matches!(
            marker_spelling(RustdocLine::from(line.trim())),
            MarkerSpelling::Bulleted
        )
    }))
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::HEADING;
    use super::marker_beside_clause;
    use super::marker_written_as_bullet;
    use crate::rustdoc::section_body;
    use crate::semantic::RustdocLine;
    use crate::semantic::SectionHeading;

    /// Fixture documentation preserves the leading space contributed by a
    /// source doc comment.
    ///
    /// # Specification
    /// trivial.
    fn specification_body(lines: &[RustdocLine<'_>]) -> Vec<String>
    {
        let block: Vec<String> = lines.iter().map(|line| format!(" {}", line.0)).collect();
        match section_body(&block, SectionHeading::from(HEADING)) {
            | Maybe::Present(lines) => lines,
            | Maybe::Absent(reason) => panic!("expected a section: {reason:?}"),
        }
    }

    #[test]
    fn the_bare_word_is_the_whole_body()
    {
        let body = specification_body(&[RustdocLine("# Specification"), RustdocLine("trivial")]);
        assert_eq!(body.len(), 1_usize, "the marker is one unfolded line");
        assert!(
            !marker_beside_clause(&body).0,
            "and the bare word alone is an accepted marker shape"
        );
        assert!(
            !marker_written_as_bullet(&body).0,
            "written without a bullet"
        );
    }

    #[test]
    fn the_marker_carries_the_terminal_period()
    {
        let body = specification_body(&[RustdocLine("# Specification"), RustdocLine("trivial.")]);
        assert!(
            !marker_beside_clause(&body).0,
            "clippy denies an unpunctuated doc paragraph, so the period is part of the marker"
        );

        let beside = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("trivial."),
            RustdocLine("- ensures: the output is sorted."),
        ]);
        assert!(
            marker_beside_clause(&beside).0,
            "and the punctuated marker conflicts with a clause exactly as the bare word does"
        );
    }

    #[test]
    fn the_marker_beside_a_clause_is_refused()
    {
        let below = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("trivial"),
            RustdocLine("- ensures: the output is sorted."),
        ]);
        assert!(
            marker_beside_clause(&below).0,
            "a clause below the marker contradicts it"
        );

        let above = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("- ensures: the output is sorted."),
            RustdocLine("trivial"),
        ]);
        assert!(
            marker_beside_clause(&above).0,
            "and a clause above the marker contradicts it just as much"
        );
    }

    #[test]
    fn clauses_without_the_marker_are_accepted()
    {
        let body = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("- requires: `value` is sorted."),
            RustdocLine("- panics: none."),
        ]);
        assert!(
            !marker_beside_clause(&body).0,
            "an ordinary clause block states its specification"
        );
        assert!(
            !marker_written_as_bullet(&body).0,
            "and no bullet of it names the marker"
        );
    }

    #[test]
    fn the_bulleted_marker_is_refused()
    {
        let bare = specification_body(&[RustdocLine("# Specification"), RustdocLine("- trivial")]);
        assert!(
            marker_written_as_bullet(&bare).0,
            "the canon carries no bullet marker, in either spelling of the word"
        );
        assert!(
            !marker_beside_clause(&bare).0,
            "and the bullet stands beside nothing, so the conflict is the bullet itself"
        );

        let punctuated =
            specification_body(&[RustdocLine("# Specification"), RustdocLine("- trivial.")]);
        assert!(
            marker_written_as_bullet(&punctuated).0,
            "the period does not make a bullet the marker"
        );

        let beside = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("- trivial"),
            RustdocLine("- ensures: the output is sorted."),
        ]);
        assert!(
            marker_beside_clause(&beside).0,
            "and a bulleted marker beside a clause is read as the marker it was meant to be"
        );
    }

    #[test]
    fn a_blank_line_is_no_clause()
    {
        let body = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine(""),
            RustdocLine("trivial"),
            RustdocLine(""),
        ]);
        assert_eq!(body.len(), 3_usize, "the body keeps its blank lines");
        assert!(
            !marker_beside_clause(&body).0,
            "and blank lines beside the marker state nothing"
        );
    }

    #[test]
    fn a_later_block_adds_no_clause_to_the_marker()
    {
        let body = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("trivial"),
            RustdocLine(""),
            RustdocLine("# Adequacy"),
            RustdocLine("- hypothesis: L3 — written below the next heading."),
        ]);
        assert!(
            !marker_beside_clause(&body).0,
            "the section ends at the next heading, so the block below it adds no clause"
        );
    }

    #[test]
    fn an_empty_section_is_no_marker_conflict()
    {
        let body = specification_body(&[RustdocLine("# Specification")]);
        assert!(body.is_empty(), "a heading with no body reads as no body");
        assert!(
            !marker_beside_clause(&body).0,
            "and presence is what this rule decides; an empty body is review's finding"
        );
    }

    #[test]
    fn a_line_merely_opening_with_the_spelling_is_a_clause()
    {
        let prose = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("trivially sorted."),
        ]);
        assert!(
            !marker_beside_clause(&prose).0,
            "the marker is the whole line, so prose opening with it is ordinary prose"
        );

        let beside = specification_body(&[
            RustdocLine("# Specification"),
            RustdocLine("trivially sorted."),
            RustdocLine("trivial"),
        ]);
        assert!(
            marker_beside_clause(&beside).0,
            "and that prose conflicts with the marker like any clause"
        );
    }
}
