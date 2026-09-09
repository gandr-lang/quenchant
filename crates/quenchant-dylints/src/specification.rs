//! The `# Specification` presence rule, checked on every function the crate
//! authors.
//!
//! The specification is authored before the implementation, so an item whose
//! rustdoc states no specification states that its author reached the body
//! without deciding what the body owes. Presence is therefore a property of the
//! source rather than a judgement about it, and a gate can hold it.
//!
//! # Presence, not grammar
//!
//! This gate answers one question — is there a block — and two refinements of
//! it, both about the canonical body `trivial.`: anything written beside the
//! word contradicts the claim that there is nothing to specify, and the word
//! behind a bullet is a clause named `trivial` rather than a claim about the
//! whole block. The clause grammar itself belongs to the specification
//! attributes and to review; the companion
//! [`crate::adequacy::ADEQUACY_BLOCK_GRAMMAR`] shows the shape a grammar gate
//! takes where one is warranted.
//!
//! # Which items the author can document
//!
//! Every function and method whose text the crate writes carries the block:
//! free functions, inherent methods, trait impl methods, provided and required
//! trait methods, foreign declarations, and functions a crate-local
//! `macro_rules!` expands. An item whose rustdoc the author cannot write is
//! exempt for that reason alone — a derive expansion, a function a macro from
//! another crate emits, and the test harness's own generated entry point are
//! nobody's prose to write. `#[test]` functions are outside the rule by
//! guidance: the test names its subject and the assertions are the statement.
//!
//! A macro from another crate reaches that exemption two ways, and the rule
//! asks about both. It can manufacture the item's name, which the name span's
//! source text answers; it can also manufacture the declaration while passing
//! the author's own identifier through into it, which the declaration's own
//! span answers. The second shape is a generated client method named after the
//! method it forwards to: the name reads as authored, and the declaration it
//! names is nobody's prose to write. Reversal: a consumer appears whose
//! generated siblings warrant documentation, which makes the declaration test
//! too broad and moves the decision to an allow-list of macro sources.

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
    /// Requires a `# Specification` rustdoc block on every function and method
    /// the crate authors, and denies a block that writes the `trivial` marker
    /// beside anything else or as a bullet.
    ///
    /// The canonical `trivial` body is `trivial.`, with the terminal period,
    /// because clippy's `doc_paragraphs_missing_punctuation`, denied in the
    /// workspace lint wall, refuses a doc paragraph that ends without one. The
    /// rule also accepts `trivial` without the period, and in either case no
    /// bullet marker and no clause stands beside it.
    ///
    /// ### Why is this bad?
    ///
    /// The specification is authored before the body it governs, so a body
    /// arriving without one records that nothing was decided about what it
    /// owes. The absence is also silent in a way the clauses are not: a wrong
    /// clause is read and argued with, while a missing block reads as an item
    /// with nothing to say. Where an item genuinely has nothing to state, the
    /// marker says exactly that and stays a judgement a reviewer can see and
    /// disagree with — which is why nothing shares a block with it. A block
    /// claiming at once that there is nothing to specify and that here is the
    /// specification has stated neither, and a bulleted `- trivial` claims
    /// neither: it reads as one oddly named clause, and the reader who wrote it
    /// believes the item is marked.
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
    /// Beyond those, an item is exempt exactly when its own syntax is not this
    /// crate's: neither its name nor its declaration is text the author wrote.
    /// That covers a function a macro from another crate emits, the entry point
    /// the test harness generates, and a method such a macro generates under
    /// the author's own identifier — its name reads as authored, and the
    /// declaration that name belongs to is nobody's prose to write. A function
    /// a crate-local `macro_rules!` expands is *not* exempt — the author writes
    /// that macro's body, so the block is written there once and every
    /// expansion carries it.
    ///
    /// Reversal: a consumer appears whose generated siblings warrant
    /// documentation, which makes the declaration test too broad and moves the
    /// decision to an allow-list of macro sources.
    ///
    /// ### Blind spots
    ///
    /// This is a presence gate, and presence is nearly all it decides. A block
    /// whose body is empty satisfies it, as does one whose clauses are
    /// misspelled or out of order: the fixed clause grammar is not read here. A
    /// block on the wrong item is likewise invisible to it — the rule asks each
    /// item for its own heading and never whether the prose beneath describes
    /// that item.
    ///
    /// The manufactured-name exemption reads the name span's source text, so it
    /// exempts the `#[doc(hidden)]` `__anodized_*` sibling a specification
    /// attribute on a trait definition synthesizes out of the author's method
    /// name, and a span the compiler has no source text for, while the author's
    /// own declaration at that same span — whose name is the text written there
    /// — stays inside the rule.
    ///
    /// The manufactured-declaration exemption reads the signature span rather
    /// than the whole item span, because rustc counts every attribute macro as
    /// external and the whole item span carries the attribute. A macro that
    /// rewrote an author's signature token by token, rather than re-emitting
    /// it, would take that author's own method out of the rule with it.
    ///
    /// ### Example
    ///
    /// ```rust
    /// /// Convert a zero-based coordinate into a one-based coordinate.
    /// fn one_based(value: ZeroBased) -> OneBased { value.successor() }
    /// ```
    ///
    /// Use instead:
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
    /// Or, where the item has nothing to state:
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

/// Late lint pass requiring the `# Specification` block.
pub struct WorkflowSpecification;

impl<'tcx> LateLintPass<'tcx> for WorkflowSpecification
{
    /// Check a free function, at its own name.
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

    /// Check an inherent or trait impl method, at its own name.
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

    /// Check a required or provided trait method, at its own name.
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

    /// Check a foreign function declaration, at its own name.
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

/// Why an item fails the `# Specification` presence rule.
enum SpecificationDefect
{
    /// The item's rustdoc carries no `# Specification` heading.
    BlockAbsent,
    /// The block writes the trivial marker beside another line.
    MarkerNotAlone,
    /// The block writes the trivial marker as a bullet.
    MarkerAsBullet,
}

impl SpecificationDefect
{
    /// Return the diagnostic text for this defect.
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

/// The heading that opens the specification block.
const HEADING: &str = "# Specification";

/// The word a block writes when there is nothing to specify.
///
/// The canonical body is `trivial.` — this word plus the terminal period the
/// prose wall asks of every doc paragraph, since clippy's
/// `doc_paragraphs_missing_punctuation` denies one that ends without it. The
/// rule also accepts the word without the period, and in either case no bullet
/// marker and no clause stands beside it. One constant, so the spelling the
/// guidance settles on stays one edit away.
const TRIVIAL_MARKER: &str = "trivial";

/// The accepted shapes, attached to every denial as its help.
const ACCEPTED_SHAPES: &str = concat!(
    "write the block as clauses in the fixed order — `- requires:`, `- ensures:`, `- provides:`, ",
    "`- fails:`, `- panics:`, `- intension:` — or, where the item has nothing to state, as the ",
    "canonical body `trivial.` with the terminal period, which the rule also accepts without it, ",
    "and in either case no bullet marker and no clause beside it",
);

/// Check the `# Specification` presence rule on one function.
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

/// Return whether an item's own syntax is syntax this crate wrote.
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

/// Return whether the source text under an item's name span is the item's own
/// identifier.
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

/// How one nonblank line of a `# Specification` body reads against the marker's
/// canon.
#[derive(Clone, Copy)]
enum MarkerSpelling
{
    /// The word alone, with or without the terminal period: the canonical body.
    Canonical,
    /// The word written as a bullet, which the canon excludes.
    Bulleted,
    /// Any other line — a clause, or prose.
    NotTheMarker,
}

/// Return how one body line reads against the marker's canon.
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

/// Return whether a section writes the trivial marker beside another line.
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

/// Return whether a section writes the trivial marker as a bullet.
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

    /// Read the `# Specification` body of a doc block as a `///` comment
    /// reaches the pass: one leading space on every line.
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
