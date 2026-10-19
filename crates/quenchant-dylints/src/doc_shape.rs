//! Module documentation keeps a shape that a reader scans and a reviewer
//! prunes.
//!
//! A crate's README is held to a byte budget and sends its detail into
//! rustdoc; these rules hold the rustdoc in turn. A module's `//!`
//! documentation is a lead paragraph, then at most `# Decisions`, `# Items`
//! and `# References`, beside the evidence headings this library's other
//! readers own: `# Specification`, `# Adequacy`, `# Termination`,
//! `# Judgement` and `# Safety`. Five rules read it, and the `rustdoc_shape`
//! group selects them together:
//!
//! - [`MODULE_DOC_HEADING`]: a heading outside that set, or a module heading
//!   written twice;
//! - [`MODULE_DOC_DECISION`]: a decision that is not one row of three bullets,
//!   a decision written outside `# Decisions`, and more than [`DECISION_CAP`]
//!   decisions in one module;
//! - [`DOC_HISTORY_PROSE`]: words that narrate a past instead of stating the
//!   present, in module and item documentation alike;
//! - [`MODULE_DOC_BUDGET`]: a crate root's documentation over
//!   [`CRATE_ROOT_BUDGET`] bytes, any other module's over [`MODULE_BUDGET`];
//! - [`MODULE_DOC_DUPLICATION`]: a paragraph of [`DUPLICATE_WORDS`] words or
//!   more that the crate's README or another module's documentation also
//!   carries.
//!
//! [`MODULE_DOC_MEASURE`] stands outside the group: it prints each documented
//! module's size and decision count, the distribution the caps are chosen
//! from. A decision row reads:
//!
//! ```text
//! - Choice: what the module does.
//! - Alternatives: what it does not do, and why not.
//! - Reversal: the condition under which the choice changes.
//! ```
//!
//! Fenced code is read as code: a `#` line inside a fence opens no heading,
//! and a history word inside a fence or a code span is an identifier.
//!
//! # Decisions
//!
//! - Choice: a row is three labelled bullets, `Choice:`, `Alternatives:` (or
//!   `Alternative:`) and `Reversal:`, in that order; the next `Choice:` opens
//!   the next row.
//! - Alternatives: three unlabelled bullets bounded by blank lines, which
//!   rustdoc renders as one list and a reflow merges; a bold lead sentence per
//!   decision, whose end no reader can find.
//! - Reversal: rustdoc gaining a definition-list or table form whose rows a
//!   reader can bound.
//! - Choice: every cap is a constant of this module, named in its lint's
//!   documentation.
//! - Alternatives: a cap in the consumer's Dylint configuration, which each
//!   landing could move and which drifts between repositories.
//! - Reversal: consumers whose documentation differs in kind, each needing a
//!   cap of its own.
//! - Choice: history prose is a fixed list of words and phrases; `used to`
//!   after a form of *be* is the present passive and passes.
//! - Alternatives: every past tense, which refuses a present statement about a
//!   past event, such as a receipt recording a landing.
//! - Reversal: a report showing the list refusing present statements or missing
//!   narration.
//! - Choice: a duplicate is the same paragraph once wrapping, emphasis, code
//!   marks and link targets are set aside.
//! - Alternatives: similarity over word sequences, which reports paraphrase as
//!   well and needs a threshold of its own.
//! - Reversal: paraphrased copies the exact comparison misses.
//! - Choice: every rule is `allow` by default; a consumer selects the group.
//! - Alternatives: `deny`, as the type and signature rules are, which refuses
//!   documentation written before the caps were agreed.
//! - Reversal: agreed caps that the consumers' documentation meets, so a `deny`
//!   costs nothing.

use alloc::collections::BTreeMap;
use std::path::PathBuf;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_and_help;
use clippy_utils::diagnostics::span_lint_hir;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use quenchant_shape::shape::Maybe;
use rustc_ast::AttrStyle;
use rustc_ast::attr::AttributeExt as _;
use rustc_hir::Attribute;
use rustc_hir::CRATE_HIR_ID;
use rustc_hir::HirId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::Level;
use rustc_middle::ty::TyCtxt;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;

use crate::rustdoc::is_indented_continuation;
use crate::rustdoc::section_lookup;
use crate::semantic::CrateRoot;
use crate::semantic::DocBytes;
use crate::semantic::HeadingLevel;
use crate::semantic::HistoryPhrase;
use crate::semantic::LineIndex;
use crate::semantic::OpensFence;
use crate::semantic::RustdocLine;
use crate::semantic::RustdocText;
use crate::semantic::WordIndex;
use crate::termination::opens_heading;

quenchant_shape::reason_enum! {
    /// A heading line can stand without a heading finding.
    mod heading_check {
        /// Why a line raises no heading finding.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Admitted {
            /// The line opens no heading, or sits inside fenced code.
            NotAHeading,
            /// The heading is a module heading written for the first time.
            ModuleHeading,
            /// The heading opens an evidence section another reader owns.
            EvidenceHeading,
            /// A sub-heading under `# Decisions` is the decision rule's.
            UnderDecisions,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A decision can be one well-formed row.
    mod row_check {
        /// Why a decision raises no row finding.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Accepted {
            /// The decision is `Choice:`, `Alternatives:`, `Reversal:`, each
            /// with a value.
            Row,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A word can open no history phrase.
    mod history_check {
        /// Why a word raises no history finding.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Present {
            /// The word opens none of the phrases that narrate a past.
            PresentTense,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Documentation can fit its budget.
    mod budget_check {
        /// Why a module's documentation raises no budget finding.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Within {
            /// The documentation is at or under its cap.
            UnderBudget,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A line can name no decision outside `# Decisions`.
    mod marker_check {
        /// Why a line carries no decision label.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unmarked {
            /// The line opens with none of the three labels.
            NoLabel,
        }
    }
}

quenchant_shape::reason_enum! {
    /// The crate's README can stay unread.
    mod readme_source {
        /// Why no README paragraph is compared.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unread {
            /// The duplication rule is not selected at the crate root.
            NotSelected,
            /// The build names no manifest directory, or it holds no readable
            /// `README.md`.
            Unreadable,
        }
    }
}

/// The decisions one module may record.
pub const DECISION_CAP: usize = 7;

/// The bytes of documentation a crate root may carry.
pub const CRATE_ROOT_BUDGET: usize = 8_usize * 1_024_usize;

/// The bytes of documentation any other module may carry.
pub const MODULE_BUDGET: usize = 4_usize * 1_024_usize;

/// The words a paragraph carries before its repetition is reported.
pub const DUPLICATE_WORDS: usize = 25;

/// The lint group the five shape rules share.
pub const GROUP: &str = "rustdoc_shape";

/// The section the decision rows live under.
const DECISIONS: &str = "# Decisions";

/// Headings a module's own documentation may open, each once.
const MODULE_HEADINGS: [&str; 3] = [DECISIONS, "# Items", "# References"];

/// Evidence headings this library's other readers own.
const EVIDENCE_HEADINGS: [&str; 5] = [
    "# Specification",
    "# Adequacy",
    "# Termination",
    "# Judgement",
    "# Safety",
];

/// A form of *be* or *get* before `used to` makes it the present passive.
const PASSIVE_AUXILIARIES: [&str; 12] = [
    "is", "are", "was", "were", "be", "been", "being", "s", "get", "gets", "got", "getting",
];

/// Words that narrate a past on their own.
const HISTORY_WORDS: [&str; 3] = ["previously", "formerly", "historically"];

/// Help attached to every decision finding.
const ROW_HELP: &str = concat!(
    "write each decision under `# Decisions` as three bullets: `- Choice:` what the module does, ",
    "`- Alternatives:` what it does not do and why, `- Reversal:` when the choice changes",
);

declare_lint! {
    /// ### What it does
    ///
    /// Reads every module's own `//!` documentation and reports each heading outside the set a module opens: `# Decisions`, `# Items` and `# References`, each at most once, beside the evidence headings `# Specification`, `# Adequacy`, `# Termination`, `# Judgement` and `# Safety`. The text before the first heading is the lead paragraph. A sub-heading under `# Decisions` is [`MODULE_DOC_DECISION`]'s to report.
    ///
    /// ### Why is this bad?
    ///
    /// A module's documentation states what the module is, the decisions it carries and what it holds. A `# Status`, `# History` or `# Notes` section narrates instead: status belongs in the tracker and history in version control, and both go stale where they stand.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// //! Parses the wire form.
    /// //!
    /// //! # Status
    /// //!
    /// //! The streaming reader is next.
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// //! Parses the wire form.
    /// ```
    pub MODULE_DOC_HEADING,
    Allow,
    "module documentation opens only its sanctioned headings"
}

declare_lint! {
    /// ### What it does
    ///
    /// Reads each module's `# Decisions` section as rows: `- Choice:`, then `- Alternatives:` (or `- Alternative:`), then `- Reversal:`, each with a value, and nothing else. It reports a decision with a fourth bullet, a missing or misplaced bullet, an empty value, an unlabelled bullet, a paragraph, a sub-heading, and a decision labelled outside the section (`Choice`, `Alternatives` or `Reversal` opening a line, bold or bulleted). A module recording more than 7 decisions ([`DECISION_CAP`]), counted in the section and outside it, is reported at its `# Decisions` heading.
    ///
    /// ### Why is this bad?
    ///
    /// A decision stated as prose grows by appending and is never pruned; a row states the choice, what it was chosen over and when it changes, and a superseded row is replaced. A module carrying more decisions than the cap is doing too much, or carries decisions that belong to another module.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// //! # Decisions
    /// //!
    /// //! **The cache is per process.** A shared cache was considered, but its
    /// //! invalidation costs more than a rebuild.
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// //! # Decisions
    /// //!
    /// //! - Choice: the cache is per process.
    /// //! - Alternatives: a shared cache, whose invalidation costs more than a
    /// //!   rebuild.
    /// //! - Reversal: a rebuild slower than the invalidation.
    /// ```
    pub MODULE_DOC_DECISION,
    Allow,
    "a module's decisions are rows of choice, alternatives and reversal, under a cap"
}

declare_lint! {
    /// ### What it does
    ///
    /// Reports the words and phrases that narrate a past instead of stating the present — `previously`, `formerly`, `historically`, `no longer`, `as of`, `used to` and `was added`, `removed`, `renamed` or `changed` `in` — in module and item documentation alike. `used to` after a form of *be* or *get* is the present passive and passes. Fenced code and code spans are not read.
    ///
    /// ### Why is this bad?
    ///
    /// Documentation states what is. A superseded statement is replaced, never annotated; history lives in version control.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// /// Reads the header. The checksum is no longer verified here.
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// /// Reads the header; the frame reader verifies the checksum.
    /// ```
    pub DOC_HISTORY_PROSE,
    Allow,
    "documentation states the present, not a history"
}

declare_lint! {
    /// ### What it does
    ///
    /// Measures each module's `//!` documentation: the bytes of its text with one byte per line break. A crate root may carry 8192 bytes ([`CRATE_ROOT_BUDGET`]), any other module 4096 ([`MODULE_BUDGET`]); the excess is reported at the documentation's first line.
    ///
    /// ### Why is this bad?
    ///
    /// A README's budget moves detail into rustdoc, where nothing prunes it. A budget per module makes growth a decision: the module splits, or a section moves to the item it describes.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    pub MODULE_DOC_BUDGET,
    Allow,
    "a module's documentation fits its byte budget"
}

declare_lint! {
    /// ### What it does
    ///
    /// Reports a paragraph of 25 words or more ([`DUPLICATE_WORDS`]) in a module's `//!` documentation that the crate's `README.md`, read from the build's manifest directory, or another module's documentation also carries. Paragraphs compare once line wrapping, emphasis, code marks and link targets are set aside; a heading, a fenced block, a table row and a link definition are not paragraphs, and each list item is one.
    ///
    /// ### Why is this bad?
    ///
    /// Two copies of a paragraph drift: one is edited, the other goes stale. The README is the overview and links into the crate's documentation, which states the detail once.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group. The README joins the crate's dependency information when the lint is selected at the crate root, so an edit to it lints the crate again.
    pub MODULE_DOC_DUPLICATION,
    Allow,
    "a paragraph is written once, in the README or in one module"
}

declare_lint! {
    /// ### What it does
    ///
    /// Prints, at the first line of each module's `//!` documentation, its bytes as [`MODULE_DOC_BUDGET`] measures them, the decisions [`MODULE_DOC_DECISION`] counts and how many of those are rows.
    ///
    /// ### Why is this bad?
    ///
    /// Nothing is wrong with a measured module. The measure is the distribution the caps are chosen from, read over a consumer by selecting the lint alone.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and outside the `rustdoc_shape` group.
    pub MODULE_DOC_MEASURE,
    Allow,
    "print each module's documentation bytes and decision count"
}

impl_lint_pass!(DocShape => [
    MODULE_DOC_HEADING,
    MODULE_DOC_DECISION,
    DOC_HISTORY_PROSE,
    MODULE_DOC_BUDGET,
    MODULE_DOC_DUPLICATION,
    MODULE_DOC_MEASURE,
]);

/// One documentation line, and whether it sits inside fenced code.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DocLine
{
    /// The line as the documentation carries it, the comment marker removed.
    text: String,
    /// The line opens, closes or sits inside a fenced code block.
    code: bool,
}

/// The documentation of one node in one attribute style.
struct DocText
{
    /// The documentation's lines, in source order.
    lines: Vec<DocLine>,
    /// The span of the attribute each line came from.
    spans: Vec<Span>,
}

/// One heading finding.
#[derive(Clone, Debug, Eq, PartialEq)]
enum HeadingDefect
{
    /// The heading is outside the sanctioned set.
    OutsideSet(String),
    /// A module heading is written a second time.
    Repeated(String),
}

/// One part of a decision, as its line opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Part
{
    /// `- Choice:`.
    Choice,
    /// `- Alternatives:` or `- Alternative:`.
    Alternatives,
    /// `- Reversal:`.
    Reversal,
    /// A bullet opening with none of the three labels.
    Unlabelled,
    /// A paragraph, or a fenced block outside every bullet.
    Prose,
    /// A heading below `# Decisions`.
    SubHeading,
}

/// One decision under `# Decisions`: where it opens and its parts.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Decision
{
    /// The line the decision opens at.
    line: usize,
    /// Each part, with whether its value states anything.
    parts: Vec<(Part, bool)>,
}

/// One way a decision fails to be a row.
#[derive(Clone, Debug, Eq, PartialEq)]
enum DecisionDefect
{
    /// A paragraph or fenced block outside every bullet.
    Prose,
    /// A heading below `# Decisions`.
    SubHeading,
    /// A bullet without a label.
    Unlabelled,
    /// The labelled bullets are not exactly the three, in order.
    NotOneRow(Vec<Part>),
    /// A labelled bullet states nothing.
    EmptyValue(Part),
}

/// What a module's `# Decisions` section and its other lines record.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DecisionReading
{
    /// The `# Decisions` heading's line, when the module has one.
    heading: Maybe<usize, section_lookup::Missing>,
    /// Each decision inside the section.
    inside: Vec<Decision>,
    /// The opening line of each decision labelled outside the section.
    outside: Vec<usize>,
}

/// The headings a module's documentation has opened, carried line to line.
#[derive(Default)]
struct HeadingState
{
    /// The module headings opened so far.
    opened: Vec<String>,
    /// A `# Decisions` section is open.
    under_decisions: bool,
}

/// The line before the one a `# Decisions` section reads.
#[derive(Clone, Copy)]
struct SectionCursor
{
    /// The line before is blank, or the section's heading.
    blank: bool,
    /// The line before is fenced code.
    code: bool,
}

/// The decisions one module records and how many are rows.
#[derive(Clone, Copy)]
struct DecisionTally
{
    /// Every decision, inside `# Decisions` and outside it.
    decisions: usize,
    /// The decisions that are rows.
    rows: usize,
}

/// One history phrase and the line it opens on.
#[derive(Clone, Debug, Eq, PartialEq)]
struct HistoryFinding
{
    /// The line of the phrase's first word.
    line: usize,
    /// The phrase, as the diagnostic names it.
    phrase: &'static str,
}

/// One paragraph: the line it opens on and its compared words.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Paragraph
{
    /// The line the paragraph opens on.
    line: usize,
    /// Its words, once wrapping, emphasis, code marks and link targets are set
    /// aside.
    words: Vec<String>,
}

/// Where a paragraph was first seen among the crate's modules.
struct FirstSeen
{
    /// The module that carries it.
    module: HirId,
    /// The span of its first line.
    span: Span,
}

/// The crate-wide reader of module documentation.
#[repr(transparent)]
pub struct DocShape
{
    /// The README's paragraphs, when the duplication rule reads them.
    readme: Maybe<Vec<Paragraph>, readme_source::Unread>,
}

impl DocShape
{
    /// Read the crate's README when the duplication rule is selected.
    ///
    /// # Specification
    /// - ensures: when [`MODULE_DOC_DUPLICATION`] is above `allow` at the crate
    ///   root and `CARGO_MANIFEST_DIR` names a directory holding a readable
    ///   `README.md`, the README is read into its paragraphs and listed among
    ///   the crate's file dependencies; otherwise no README is read.
    /// - provides: `readme_source::Unread::NotSelected` when the rule is
    ///   `allow`; `Unreadable` when the directory or the file is missing.
    /// - panics: none.
    /// - executable: none — the file is read from outside the compilation;
    ///   reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the duplication fixture names a README directory of
    ///   its own and is reported a paragraph that README carries; the other
    ///   fixtures select no duplication rule and read none.
    /// - witness: `tests::ui_doc_shape`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let level = tcx
            .lint_level_spec_at_node(MODULE_DOC_DUPLICATION, CRATE_HIR_ID)
            .level();
        if matches!(level, Level::Allow) {
            return Self {
                readme: Maybe::Absent(readme_source::Unread::NotSelected),
            };
        }
        let Some(directory) = std::env::var_os("CARGO_MANIFEST_DIR")
        else {
            return Self {
                readme: Maybe::Absent(readme_source::Unread::Unreadable),
            };
        };
        let path = PathBuf::from(directory).join("README.md");
        let Ok(text) = std::fs::read_to_string(&path)
        else {
            return Self {
                readme: Maybe::Absent(readme_source::Unread::Unreadable),
            };
        };
        tcx.sess
            .file_depinfo
            .lock()
            .insert(Symbol::intern(&path.to_string_lossy()));
        Self {
            readme: Maybe::Present(paragraphs(&classify(RustdocText::from(text.as_str())))),
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for DocShape
{
    /// Read every node's documentation for history prose.
    ///
    /// # Specification
    /// - ensures: reports [`DOC_HISTORY_PROSE`] at the attribute carrying each
    ///   phrase [`history_findings`] finds in the node's outer documentation
    ///   and, separately, in its inner documentation; documentation an external
    ///   macro wrote is not read.
    /// - panics: none.
    /// - executable: none — rustc owns the diagnostic sink; this callback has
    ///   no readback of emitted or suppressed diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the history fixture carries each phrase in item and
    ///   module documentation, a phrase wrapped across two lines, the passive
    ///   `is used to`, and the words inside a code span and a fence, the last
    ///   three not reported.
    /// - witness: `tests::ui_doc_shape`
    fn check_attributes(
        &mut self,
        cx: &LateContext<'tcx>,
        attrs: &'tcx [Attribute],
    )
    {
        for style in [AttrStyle::Outer, AttrStyle::Inner] {
            let text = doc_text(cx, attrs, style);
            for finding in history_findings(&text.lines) {
                let Some(&span) = text.spans.get(finding.line)
                else {
                    continue;
                };
                span_lint_and_help(
                    cx,
                    DOC_HISTORY_PROSE,
                    span,
                    format!(
                        "`{}` narrates a history; documentation states the present",
                        finding.phrase
                    ),
                    None,
                    "replace a superseded statement with the current one; history lives in \
                     version control",
                );
            }
        }
    }

    /// Judge every module's own documentation once the crate is read.
    ///
    /// # Specification
    /// - ensures: for the crate root and each module, reads its inner
    ///   documentation and reports, at the module, each heading
    ///   [`heading_judgement`] refuses, each decision [`row_judgement`]
    ///   refuses, each decision labelled outside `# Decisions`, a decision
    ///   count over [`DECISION_CAP`], documentation over its budget, and each
    ///   paragraph of [`DUPLICATE_WORDS`] words or more the README or an
    ///   earlier module carries; then prints [`MODULE_DOC_MEASURE`] for every
    ///   documented module.
    /// - panics: none.
    /// - executable: none — rustc owns the diagnostic sink; this callback has
    ///   no readback of emitted or suppressed diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — one fixture per refusal carries the refused shapes at
    ///   a crate root and in modules, and the accepted fixture carries every
    ///   sanctioned heading, rows at the cap, documentation at a module's
    ///   budget and a repeated short paragraph, none reported.
    /// - witness: `tests::ui_doc_shape`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let mut seen: BTreeMap<Vec<String>, FirstSeen> = BTreeMap::new();
        cx.tcx.hir_for_each_module(|module| {
            let def_id = module.to_local_def_id();
            let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
            let root = CrateRoot(hir_id == CRATE_HIR_ID);
            let place = if root.0 { "crate root" } else { "module" };
            let text = doc_text(cx, cx.tcx.hir_attrs(hir_id), AttrStyle::Inner);
            let Some(&first) = text.spans.first()
            else {
                return;
            };
            report_headings(cx, hir_id, &text, first);
            let reading = read_decisions(&text.lines);
            let tally = report_decisions(cx, hir_id, &reading, &text, first);
            let bytes = doc_bytes(&text.lines);
            if let Maybe::Present(excess) = budget_judgement(bytes, root) {
                let (place, cap) = if root.0 {
                    ("crate root", CRATE_ROOT_BUDGET)
                }
                else {
                    ("module", MODULE_BUDGET)
                };
                span_lint_hir_and_then(
                    cx,
                    MODULE_DOC_BUDGET,
                    hir_id,
                    first,
                    format!(
                        "this {place}'s documentation is {} bytes, over its {cap}-byte budget",
                        excess.0
                    ),
                    |diag| {
                        diag.help(
                            "split the module, or move a section to the item it describes; \
                             never compress the prose to fit",
                        );
                    },
                );
            }
            self.report_duplicates(cx, hir_id, &text, &mut seen);
            span_lint_hir(
                cx,
                MODULE_DOC_MEASURE,
                hir_id,
                first,
                format!(
                    "{place} documentation: {} bytes, {} decisions, {} rows",
                    bytes.0, tally.decisions, tally.rows
                ),
            );
        });
    }
}

impl DocShape
{
    /// Report each paragraph the README or an earlier module also carries.
    ///
    /// # Specification
    /// - requires: `text` is the inner documentation of the module `hir_id`,
    ///   and `seen` holds the paragraphs of the modules judged before it.
    /// - ensures: for each paragraph of [`DUPLICATE_WORDS`] words or more,
    ///   reports [`MODULE_DOC_DUPLICATION`] at its first line when the README
    ///   carries it, naming the README line; otherwise when another module
    ///   carries it, noting that module's line; otherwise records it in `seen`.
    /// - panics: none.
    /// - executable: none — rustc owns the diagnostic sink; this callback has
    ///   no readback of emitted or suppressed diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the duplication fixture carries a paragraph its
    ///   README carries, rewrapped and with its link targets removed, and a
    ///   paragraph two modules carry, each reported; a paragraph one word short
    ///   of the threshold is repeated and not reported.
    /// - witness: `tests::ui_doc_shape`
    fn report_duplicates(
        &self,
        cx: &LateContext<'_>,
        hir_id: HirId,
        text: &DocText,
        seen: &mut BTreeMap<Vec<String>, FirstSeen>,
    )
    {
        for paragraph in paragraphs(&text.lines) {
            if paragraph.words.len() < DUPLICATE_WORDS {
                continue;
            }
            let Some(&span) = text.spans.get(paragraph.line)
            else {
                continue;
            };
            let opening = paragraph
                .words
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<String>>()
                .join(" ");
            if let Maybe::Present(ref readme) = self.readme
                && let Some(original) = readme
                    .iter()
                    .find(|candidate| candidate.words == paragraph.words)
            {
                span_lint_hir_and_then(
                    cx,
                    MODULE_DOC_DUPLICATION,
                    hir_id,
                    span,
                    format!("this paragraph repeats the crate's README: \"{opening} …\""),
                    |diag| {
                        diag.note(format!(
                            "`README.md` line {} carries the same paragraph",
                            original.line.saturating_add(1)
                        ));
                        diag.help("state the paragraph once, and link to it from the other");
                    },
                );
                continue;
            }
            if let Some(first) = seen.get(&paragraph.words) {
                if first.module != hir_id {
                    span_lint_hir_and_then(
                        cx,
                        MODULE_DOC_DUPLICATION,
                        hir_id,
                        span,
                        format!(
                            "this paragraph repeats another module's documentation: \"{opening} …\""
                        ),
                        |diag| {
                            diag.span_note(first.span, "the same paragraph");
                            diag.help("state the paragraph once, and link to it from the other");
                        },
                    );
                }
                continue;
            }
            let _fresh = seen.insert(paragraph.words, FirstSeen {
                module: hir_id,
                span,
            });
        }
    }
}

/// Report each heading finding of one module.
///
/// # Specification
/// - requires: `text` is the inner documentation of the module `hir_id`, and
///   `first` the span of its first line.
/// - ensures: reports [`MODULE_DOC_HEADING`] at each line [`heading_judgement`]
///   refuses, in source order.
/// - panics: none.
/// - executable: none — rustc owns the diagnostic sink; this function has no
///   readback of emitted or suppressed diagnostics.
///
/// # Adequacy
/// - hypothesis: L3 — the heading fixture carries headings outside the set at
///   two levels, a repeated module heading and a `#` line inside a fence, the
///   last not reported.
/// - witness: `tests::ui_doc_shape`
fn report_headings(
    cx: &LateContext<'_>,
    hir_id: HirId,
    text: &DocText,
    first: Span,
)
{
    let mut state = HeadingState::default();
    for (index, line) in text.lines.iter().enumerate() {
        let Maybe::Present(defect) = heading_judgement(line, &mut state)
        else {
            continue;
        };
        let span = text.spans.get(index).copied().unwrap_or(first);
        let message = match defect {
            HeadingDefect::OutsideSet(heading) => {
                format!("`{heading}` is not a module documentation heading")
            },
            HeadingDefect::Repeated(heading) => {
                format!("`{heading}` opens twice in this module's documentation")
            },
        };
        span_lint_hir_and_then(cx, MODULE_DOC_HEADING, hir_id, span, message, |diag| {
            diag.help(
                "a module's documentation is a lead paragraph, then at most `# Decisions`, \
                 `# Items` and `# References`, beside the evidence sections `# Specification`, \
                 `# Adequacy`, `# Termination`, `# Judgement` and `# Safety`; status belongs in \
                 the tracker, history in version control",
            );
        });
    }
}

/// Report each decision finding of one module and count its rows.
///
/// # Specification
/// - requires: `reading` is [`read_decisions`] over `text`, the inner
///   documentation of the module `hir_id`, and `first` the span of its first
///   line.
/// - ensures: reports [`MODULE_DOC_DECISION`] at each decision
///   [`row_judgement`] refuses, at each decision labelled outside the section,
///   and at the section's heading (or the first line) when the decisions
///   outnumber [`DECISION_CAP`]; returns the number of decisions, a sub-heading
///   not counted, and of rows.
/// - panics: none.
/// - executable: none — rustc owns the diagnostic sink; this function has no
///   readback of emitted or suppressed diagnostics.
///
/// # Adequacy
/// - hypothesis: L3 — the decision fixture carries each way a decision fails to
///   be a row, a labelled decision outside the section, and a module over the
///   cap; the accepted fixture carries a module at the cap.
/// - witness: `tests::ui_doc_shape`
fn report_decisions(
    cx: &LateContext<'_>,
    hir_id: HirId,
    reading: &DecisionReading,
    text: &DocText,
    first: Span,
) -> DecisionTally
{
    let span_of = |line: usize| text.spans.get(line).copied().unwrap_or(first);
    let mut rows = 0_usize;
    let mut decisions = reading.outside.len();
    for decision in &reading.inside {
        let defect = match row_judgement(decision) {
            Maybe::Absent(row_check::Accepted::Row) => {
                rows = rows.saturating_add(1);
                decisions = decisions.saturating_add(1);
                continue;
            },
            Maybe::Present(defect) => defect,
        };
        let message = match defect {
            DecisionDefect::Prose => "prose under `# Decisions` is not a decision row".to_owned(),
            DecisionDefect::SubHeading => {
                "a sub-heading under `# Decisions` is not a decision row".to_owned()
            },
            DecisionDefect::Unlabelled => concat!(
                "this decision is not a row: its bullet opens with none of `Choice:`, ",
                "`Alternatives:` and `Reversal:`"
            )
            .to_owned(),
            DecisionDefect::NotOneRow(parts) => format!(
                "this decision is not one row of three bullets: it reads {}",
                part_names(&parts)
            ),
            DecisionDefect::EmptyValue(part) => format!(
                "this decision's {} bullet states nothing",
                part_names(&[part])
            ),
        };
        if !matches!(decision.parts.first(), Some(&(Part::SubHeading, _))) {
            decisions = decisions.saturating_add(1);
        }
        span_lint_hir_and_then(
            cx,
            MODULE_DOC_DECISION,
            hir_id,
            span_of(decision.line),
            message,
            |diag| {
                diag.help(ROW_HELP);
            },
        );
    }
    for &line in &reading.outside {
        span_lint_hir_and_then(
            cx,
            MODULE_DOC_DECISION,
            hir_id,
            span_of(line),
            "this decision stands outside `# Decisions`",
            |diag| {
                diag.help(ROW_HELP);
            },
        );
    }
    if decisions > DECISION_CAP {
        let line = match reading.heading {
            Maybe::Present(line) => line,
            Maybe::Absent(_) => 0_usize,
        };
        span_lint_hir_and_then(
            cx,
            MODULE_DOC_DECISION,
            hir_id,
            span_of(line),
            format!("this module records {decisions} decisions, over the cap of {DECISION_CAP}"),
            |diag| {
                diag.help(
                    "a module carrying more decisions than the cap is doing too much, or carries \
                     decisions that belong to another module",
                );
            },
        );
    }
    DecisionTally { decisions, rows }
}

/// Name a run of decision parts as their labels.
///
/// # Specification
/// trivial.
fn part_names(parts: &[Part]) -> String
{
    parts
        .iter()
        .map(|part| match *part {
            Part::Choice => "`Choice:`",
            Part::Alternatives => "`Alternatives:`",
            Part::Reversal => "`Reversal:`",
            Part::Unlabelled => "an unlabelled bullet",
            Part::Prose => "a paragraph",
            Part::SubHeading => "a sub-heading",
        })
        .collect::<Vec<&str>>()
        .join(", ")
}

/// A node's documentation in one attribute style, line by line.
///
/// # Specification
/// - ensures: returns, in source order, every line of every documentation
///   attribute of `style` among `attrs` that no external macro wrote, each
///   paired with its attribute's span — the invocation's, for an attribute a
///   local macro expanded — and classified by [`classify`]; an empty
///   documentation line is kept as an empty line.
/// - provides: the input every rule of this module reads.
/// - panics: none.
/// - executable: none — attribute provenance is the compiler's; the UI fixtures
///   are the oracle.
///
/// # Adequacy
/// - hypothesis: L2 — the fixtures distinguish inner from outer documentation
///   on one module and an empty line from a paragraph's last, and report a
///   comment a local macro carries at the macro's invocation.
/// - witness: `tests::ui_doc_shape`
fn doc_text(
    cx: &LateContext<'_>,
    attrs: &[Attribute],
    style: AttrStyle,
) -> DocText
{
    let mut raw = String::new();
    let mut spans = Vec::new();
    for attr in attrs {
        if attr.doc_resolution_scope() != Some(style) {
            continue;
        }
        let Some(comment) = attr.doc_str()
        else {
            continue;
        };
        let written = attr.span();
        if written.in_external_macro(cx.tcx.sess.source_map()) {
            continue;
        }
        let span = if written.from_expansion() {
            written.source_callsite()
        }
        else {
            written
        };
        for line in comment.as_str().split('\n') {
            if !spans.is_empty() {
                raw.push('\n');
            }
            raw.push_str(line.strip_suffix('\r').unwrap_or(line));
            spans.push(span);
        }
    }
    let lines = if spans.is_empty() {
        Vec::new()
    }
    else {
        classify(RustdocText::from(raw.as_str()))
    };
    DocText { lines, spans }
}

/// Lines of documentation text, each marked as code or prose.
///
/// # Specification
/// - ensures: returns one line per `\n`-separated line of `text`, in order; a
///   line is code exactly when it opens or closes a fence (its trimmed text
///   starts with three backticks or three tildes) or sits between an opening
///   fence and its close.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a `#` line inside a fence, the fence
///   lines themselves, and the prose after the closing fence.
/// - witness: `doc_shape::tests::a_fenced_line_is_neither_heading_nor_prose`
#[spec(ensures: |output| output.len() == text.0.split('\n').count())]
fn classify(text: RustdocText<'_>) -> Vec<DocLine>
{
    let mut fenced = false;
    text.0
        .split('\n')
        .map(|line| {
            let fence = opens_fence(RustdocLine::from(line)).0;
            let code = fenced || fence;
            if fence {
                fenced = !fenced;
            }
            DocLine {
                text: line.to_owned(),
                code,
            }
        })
        .collect()
}

/// A fence delimiter opens or closes fenced code.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the trimmed line starts with
///   three backticks or three tildes.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a backtick fence with an info string,
///   and a code span that opens a line.
/// - witness: `doc_shape::tests::a_fenced_line_is_neither_heading_nor_prose`
#[spec(ensures: |output| output.0 == (line.0.trim_start().starts_with("```") || line.0.trim_start().starts_with("~~~")))]
fn opens_fence(line: RustdocLine<'_>) -> OpensFence
{
    let trimmed = line.0.trim_start();
    OpensFence(trimmed.starts_with("```") || trimmed.starts_with("~~~"))
}

/// The heading level a line opens, zero for none.
///
/// # Specification
/// - ensures: returns the length of the line's leading `#` run when the trimmed
///   line opens a heading under [`opens_heading`], zero otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a `#` heading, a `##` heading and a
///   `#` run without its space.
/// - witness: `doc_shape::tests::a_fenced_line_is_neither_heading_nor_prose`
#[spec(ensures: |output| (output.0 == 0) != opens_heading(RustdocLine::from(line.0.trim())).0)]
fn heading_level(line: RustdocLine<'_>) -> HeadingLevel
{
    let trimmed = line.0.trim();
    if !opens_heading(RustdocLine::from(trimmed)).0 {
        return HeadingLevel(0);
    }
    HeadingLevel(
        trimmed
            .chars()
            .take_while(|&character| character == '#')
            .count(),
    )
}

/// Judge one line of a module's documentation as a heading.
///
/// # Specification
/// - requires: the lines of one module are judged in order, with `state`
///   carried from line to line and starting empty.
/// - ensures: refuses a heading outside [`MODULE_HEADINGS`] and
///   [`EVIDENCE_HEADINGS`], and a module heading `state` has opened already;
///   admits a line that opens no heading or sits in fenced code, and a
///   sub-heading while a `# Decisions` section is open.
/// - provides: `heading_check::Admitted` names why the line is admitted.
/// - panics: none.
/// - executable: none — the judgement threads mutable state across calls; the
///   UI fixtures are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate every admitted reason and both
///   refusals.
/// - witness: `doc_shape::tests::headings_are_judged_against_the_set`
fn heading_judgement(
    line: &DocLine,
    state: &mut HeadingState,
) -> Maybe<HeadingDefect, heading_check::Admitted>
{
    let level = heading_level(RustdocLine::from(&line.text)).0;
    if line.code || level == 0 {
        return Maybe::Absent(heading_check::Admitted::NotAHeading);
    }
    if level > 1 && state.under_decisions {
        return Maybe::Absent(heading_check::Admitted::UnderDecisions);
    }
    let heading = line.text.trim();
    state.under_decisions = heading == DECISIONS;
    if MODULE_HEADINGS.contains(&heading) {
        if state.opened.iter().any(|seen| seen == heading) {
            return Maybe::Present(HeadingDefect::Repeated(heading.to_owned()));
        }
        state.opened.push(heading.to_owned());
        return Maybe::Absent(heading_check::Admitted::ModuleHeading);
    }
    if EVIDENCE_HEADINGS.contains(&heading) {
        return Maybe::Absent(heading_check::Admitted::EvidenceHeading);
    }
    Maybe::Present(HeadingDefect::OutsideSet(heading.to_owned()))
}

/// The decisions a module's documentation records, inside `# Decisions` and
/// out.
///
/// # Specification
/// - ensures: the section runs from the first `# Decisions` heading outside
///   fenced code to the next level-one heading; inside it, a `- Choice:`
///   bullet, an unlabelled bullet, a paragraph, an unindented fence and a
///   sub-heading each open a decision, an `- Alternatives:` or `- Reversal:`
///   bullet joins the open decision (opening one when none is open), and an
///   indented line continues the bullet above it. Outside it, each line
///   [`decision_label`] marks a `Reversal` closes a decision opened at the last
///   unclosed `Choice` (or at itself), and a `Choice` left unclosed by the next
///   `Choice` or the end is a decision of its own.
/// - panics: none.
/// - executable: none — the reading is a fold over the lines whose statement
///   restates it; the tests and the UI fixtures are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a row, a row with a wrapped value, a
///   fourth bullet, a bullet out of order, an unlabelled bullet, a paragraph, a
///   sub-heading, and labelled decisions outside the section with and without
///   their `Choice`.
/// - witness: `doc_shape::tests::a_row_is_three_labelled_bullets_in_order`
/// - witness: `doc_shape::tests::decisions_outside_the_section_are_counted`
fn read_decisions(lines: &[DocLine]) -> DecisionReading
{
    let start = lines
        .iter()
        .position(|line| !line.code && line.text.trim() == DECISIONS);
    let end = start.map_or(0_usize, |start| {
        lines
            .iter()
            .enumerate()
            .skip(start.saturating_add(1))
            .find(|&(_, line)| !line.code && heading_level(RustdocLine::from(&line.text)).0 == 1)
            .map_or(lines.len(), |(index, _)| index)
    });
    let mut inside: Vec<Decision> = Vec::new();
    let mut outside = Vec::new();
    let mut open_choice: Option<usize> = None;
    let mut previous = SectionCursor {
        blank: true,
        code: false,
    };
    for (index, line) in lines.iter().enumerate() {
        if start.is_some_and(|start| index > start && index < end) {
            read_section_line(line, LineIndex(index), &mut inside, previous);
            previous = SectionCursor {
                blank: line.text.trim().is_empty(),
                code: line.code,
            };
            continue;
        }
        if line.code || Some(index) == start {
            continue;
        }
        match decision_label(RustdocLine::from(&line.text)) {
            Maybe::Present(Part::Choice) => {
                outside.extend(open_choice.replace(index));
            },
            Maybe::Present(Part::Reversal) => {
                outside.push(open_choice.take().unwrap_or(index));
            },
            Maybe::Present(
                Part::Alternatives | Part::Unlabelled | Part::Prose | Part::SubHeading,
            )
            | Maybe::Absent(marker_check::Unmarked::NoLabel) => {},
        }
    }
    outside.extend(open_choice);
    DecisionReading {
        heading: start.map_or(
            Maybe::Absent(section_lookup::Missing::HeadingAbsent),
            Maybe::Present,
        ),
        inside,
        outside,
    }
}

/// Fold one line of a `# Decisions` section into its decisions.
///
/// # Specification
/// - requires: `line` is line `index` of the section, and `previous` describes
///   the line before it.
/// - ensures: as [`read_decisions`] states for a line inside the section.
/// - panics: none.
/// - executable: none — the fold mutates the decision list it extends; the
///   tests are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — as [`read_decisions`].
/// - witness: `doc_shape::tests::a_row_is_three_labelled_bullets_in_order`
fn read_section_line(
    line: &DocLine,
    index: LineIndex,
    inside: &mut Vec<Decision>,
    previous: SectionCursor,
)
{
    let index = index.0;
    let trimmed = line.text.trim();
    let continuation = is_indented_continuation(RustdocLine::from(&line.text)).0;
    if line.code {
        if previous.code || continuation {
            return;
        }
        inside.push(Decision {
            line: index,
            parts: vec![(Part::Prose, true)],
        });
        return;
    }
    if trimmed.is_empty() {
        return;
    }
    if heading_level(RustdocLine::from(trimmed)).0 > 1 {
        inside.push(Decision {
            line: index,
            parts: vec![(Part::SubHeading, true)],
        });
        return;
    }
    if continuation && let Some(open) = inside.last_mut() {
        if let Some(&mut (Part::Choice | Part::Alternatives | Part::Reversal, ref mut stated)) =
            open.parts.last_mut()
        {
            *stated = true;
        }
        return;
    }
    let Some(bullet) = trimmed.strip_prefix("- ")
    else {
        let continues_prose = !previous.blank
            && inside
                .last()
                .is_some_and(|open| matches!(open.parts.last(), Some(&(Part::Prose, _))));
        if !continues_prose {
            inside.push(Decision {
                line: index,
                parts: vec![(Part::Prose, true)],
            });
        }
        return;
    };
    let (part, value) = labelled(RustdocLine::from(bullet));
    let stated = !value.0.trim().is_empty();
    let joins = matches!(part, Part::Alternatives | Part::Reversal)
        && inside.last().is_some_and(|open| {
            matches!(
                open.parts.first(),
                Some(&(Part::Choice | Part::Alternatives | Part::Reversal, _))
            )
        });
    if joins && let Some(open) = inside.last_mut() {
        open.parts.push((part, stated));
        return;
    }
    inside.push(Decision {
        line: index,
        parts: vec![(part, stated)],
    });
}

/// A bullet's label and the value after it.
///
/// # Specification
/// - requires: `bullet` is a bullet's text after its `- ` marker.
/// - ensures: returns `Choice`, `Alternatives` or `Reversal` with the text
///   after the label when the bullet opens with `Choice:`, `Alternatives:` or
///   `Alternative:`, or `Reversal:`; otherwise `Unlabelled` with the whole
///   bullet.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate each label, the singular alternative,
///   and a bold lead.
/// - witness: `doc_shape::tests::a_row_is_three_labelled_bullets_in_order`
#[spec(ensures: |output| matches!(output.0, Part::Unlabelled) == (output.1 == bullet))]
fn labelled(bullet: RustdocLine<'_>) -> (Part, RustdocLine<'_>)
{
    for (label, part) in [
        ("Choice:", Part::Choice),
        ("Alternatives:", Part::Alternatives),
        ("Alternative:", Part::Alternatives),
        ("Reversal:", Part::Reversal),
    ] {
        if let Some(value) = bullet.0.strip_prefix(label) {
            return (part, RustdocLine::from(value));
        }
    }
    (Part::Unlabelled, bullet)
}

/// The decision label a line outside `# Decisions` opens with.
///
/// # Specification
/// - ensures: returns `Choice`, `Alternatives` or `Reversal` exactly when the
///   trimmed line, after an optional `- ` or `* ` bullet and optional `**` or
///   `*` emphasis, opens with `Choice`, `Alternative`, `Alternatives` or
///   `Reversal` followed by `:` or `.`.
/// - provides: `marker_check::Unmarked::NoLabel` for every other line.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a plain, a bulleted and a bold label,
///   a label followed by a word, and the word inside a sentence.
/// - witness: `doc_shape::tests::decisions_outside_the_section_are_counted`
#[spec(ensures: |output| matches!(output, Maybe::Absent(_))
    || ["Choice", "Alternative", "Reversal"].iter().any(|label| line.0.contains(label)))]
fn decision_label(line: RustdocLine<'_>) -> Maybe<Part, marker_check::Unmarked>
{
    let trimmed = line.0.trim();
    let unbulleted = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed);
    let plain = unbulleted.trim_start_matches('*');
    for (label, part) in [
        ("Choice", Part::Choice),
        ("Alternatives", Part::Alternatives),
        ("Alternative", Part::Alternatives),
        ("Reversal", Part::Reversal),
    ] {
        if plain
            .strip_prefix(label)
            .is_some_and(|rest| rest.starts_with(':') || rest.starts_with('.'))
        {
            return Maybe::Present(part);
        }
    }
    Maybe::Absent(marker_check::Unmarked::NoLabel)
}

/// Judge one decision as a row.
///
/// # Specification
/// - ensures: accepts exactly `Choice`, `Alternatives`, `Reversal`, in that
///   order, each stating a value; otherwise refuses, in this order of
///   precedence, a decision opening with prose or a sub-heading, one carrying
///   an unlabelled bullet, one whose labels are not exactly the three in order,
///   and one whose first empty bullet is named.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a row and every refusal.
/// - witness: `doc_shape::tests::a_row_is_three_labelled_bullets_in_order`
#[spec(ensures: |output| matches!(output, Maybe::Absent(_)) == matches!(
    decision.parts.as_slice(),
    [(Part::Choice, true), (Part::Alternatives, true), (Part::Reversal, true)]
))]
fn row_judgement(decision: &Decision) -> Maybe<DecisionDefect, row_check::Accepted>
{
    let parts: Vec<Part> = decision.parts.iter().map(|&(part, _)| part).collect();
    match parts.first() {
        Some(&Part::Prose) => return Maybe::Present(DecisionDefect::Prose),
        Some(&Part::SubHeading) => return Maybe::Present(DecisionDefect::SubHeading),
        Some(&(Part::Choice | Part::Alternatives | Part::Reversal | Part::Unlabelled)) | None => {},
    }
    if parts.contains(&Part::Unlabelled) {
        return Maybe::Present(DecisionDefect::Unlabelled);
    }
    if parts != [Part::Choice, Part::Alternatives, Part::Reversal] {
        return Maybe::Present(DecisionDefect::NotOneRow(parts));
    }
    if let Some(&(part, _)) = decision.parts.iter().find(|&&(_, stated)| !stated) {
        return Maybe::Present(DecisionDefect::EmptyValue(part));
    }
    Maybe::Absent(row_check::Accepted::Row)
}

/// The bytes a module's documentation carries.
///
/// # Specification
/// - ensures: returns the bytes of every line's text plus one per line break,
///   which is the documentation's text as one string with a final newline.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests measure an empty line, a multi-byte character
///   and the budget's cap.
/// - witness: `doc_shape::tests::the_budget_admits_its_cap_and_refuses_one_byte_more`
#[spec(ensures: |output| output.0 == lines.iter().map(|line| line.text.len()).sum::<usize>().saturating_add(lines.len()))]
fn doc_bytes(lines: &[DocLine]) -> DocBytes
{
    DocBytes(lines.iter().fold(0_usize, |bytes, line| {
        bytes.saturating_add(line.text.len()).saturating_add(1)
    }))
}

/// Judge a module's documentation bytes against its budget.
///
/// # Specification
/// - ensures: refuses with the bytes exactly when they exceed
///   [`CRATE_ROOT_BUDGET`] at a crate root or [`MODULE_BUDGET`] elsewhere.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate each cap, one byte over each, and a
///   module's size at a crate root.
/// - witness: `doc_shape::tests::the_budget_admits_its_cap_and_refuses_one_byte_more`
#[spec(ensures: |output| matches!(output, Maybe::Present(_))
    == (bytes.0 > (if root.0 { CRATE_ROOT_BUDGET } else { MODULE_BUDGET })))]
fn budget_judgement(
    bytes: DocBytes,
    root: CrateRoot,
) -> Maybe<DocBytes, budget_check::Within>
{
    let cap = if root.0 {
        CRATE_ROOT_BUDGET
    }
    else {
        MODULE_BUDGET
    };
    if bytes.0 > cap {
        return Maybe::Present(bytes);
    }
    Maybe::Absent(budget_check::Within::UnderBudget)
}

/// The words of documentation prose, each with its line.
///
/// # Specification
/// - ensures: returns, in order, every maximal run of alphanumeric characters
///   on a line outside fenced code, lowercased; a code span (text between
///   backticks on one line) is left out and stands as one backtick word, so the
///   words either side of it never join into a phrase.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — as [`history_findings`].
/// - witness: `doc_shape::tests::used_to_after_a_form_of_be_is_the_present`
/// - witness: `doc_shape::tests::a_phrase_wrapped_across_lines_is_found`
#[spec(ensures: |output| output.iter().all(|(word, line)| !word.is_empty() && line.0 < lines.len()))]
fn prose_words(lines: &[DocLine]) -> Vec<(String, LineIndex)>
{
    let mut words = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.code {
            continue;
        }
        let mut in_span = false;
        let mut word = String::new();
        for character in line.text.chars().chain(core::iter::once(' ')) {
            if character == '`' {
                if !word.is_empty() {
                    words.push((core::mem::take(&mut word), LineIndex(index)));
                }
                if !in_span {
                    words.push(("`".to_owned(), LineIndex(index)));
                }
                in_span = !in_span;
                continue;
            }
            if !in_span && character.is_alphanumeric() {
                word.extend(character.to_lowercase());
                continue;
            }
            if !word.is_empty() {
                words.push((core::mem::take(&mut word), LineIndex(index)));
            }
        }
    }
    words
}

/// The history phrases a node's documentation carries.
///
/// # Specification
/// - ensures: returns, in order, one finding at the line of the first word of
///   each phrase [`history_phrase`] reads at a word of [`prose_words`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate each phrase, the passive `used to`, a
///   phrase wrapped across lines, and the words in a code span and a fence.
/// - witness: `doc_shape::tests::used_to_after_a_form_of_be_is_the_present`
/// - witness: `doc_shape::tests::a_phrase_wrapped_across_lines_is_found`
#[spec(ensures: |output| output.iter().all(|finding| finding.line < lines.len())
    && output.is_sorted_by_key(|finding| finding.line))]
fn history_findings(lines: &[DocLine]) -> Vec<HistoryFinding>
{
    let words = prose_words(lines);
    words
        .iter()
        .enumerate()
        .filter_map(
            |(index, &(_, line))| match history_phrase(&words, WordIndex(index)) {
                Maybe::Present(phrase) => Some(HistoryFinding {
                    line: line.0,
                    phrase: phrase.0,
                }),
                Maybe::Absent(history_check::Present::PresentTense) => None,
            },
        )
        .collect()
}

/// The history phrase opening at one word, if any.
///
/// # Specification
/// - requires: `words` are a node's prose words in order and `index` one of
///   their positions.
/// - ensures: returns the phrase when the word at `index` is one of
///   [`HISTORY_WORDS`]; opens `no longer` not followed by `than`, or `as of`;
///   opens `used to` and the word before it is none of [`PASSIVE_AUXILIARIES`];
///   or opens `was`, then `added`, `removed`, `renamed` or `changed`, then
///   `in`.
/// - provides: `history_check::Present::PresentTense` for every other word.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate each phrase, `used to` after each kind
///   of auxiliary and after none, `was` before another verb, the comparison `no
///   longer than`, and `as` and `of` either side of a code span.
/// - witness: `doc_shape::tests::used_to_after_a_form_of_be_is_the_present`
#[spec(ensures: |output| matches!(output, Maybe::Absent(_)) || index.0 < words.len())]
fn history_phrase(
    words: &[(String, LineIndex)],
    index: WordIndex,
) -> Maybe<HistoryPhrase<'static>, history_check::Present>
{
    let word = |position: Option<usize>| {
        position
            .and_then(|position| words.get(position))
            .map_or("", |&(ref spelled, _)| spelled.as_str())
    };
    let at = |offset: usize| word(index.0.checked_add(offset));
    if let Some(&single) = HISTORY_WORDS.iter().find(|&&single| single == at(0)) {
        return Maybe::Present(HistoryPhrase(single));
    }
    let phrase = match (at(0), at(1), at(2)) {
        ("no", "longer", following) if following != "than" => "no longer",
        ("as", "of", _) => "as of",
        ("used", "to", _) if !PASSIVE_AUXILIARIES.contains(&word(index.0.checked_sub(1))) => {
            "used to"
        },
        ("was", "added", "in") => "was added in",
        ("was", "removed", "in") => "was removed in",
        ("was", "renamed", "in") => "was renamed in",
        ("was", "changed", "in") => "was changed in",
        _ => return Maybe::Absent(history_check::Present::PresentTense),
    };
    Maybe::Present(HistoryPhrase(phrase))
}

/// The paragraphs of documentation or README text.
///
/// # Specification
/// - ensures: returns, in order, each maximal run of prose lines, where a blank
///   line, a heading, a fenced line, a table row (`|`), a link definition
///   (`[…]:`) and an HTML line (`<`) separate runs and are no paragraph, and a
///   line opening a list item (`- `, `* `, or digits and `. `) opens a run of
///   its own; each paragraph's words are its text once list markers, emphasis
///   (`*`), code marks, brackets and link targets (`(…)` or `[…]` directly
///   after `]`) are set aside, split at whitespace.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a wrapped paragraph from its one-line
///   twin, a linked word from its plain twin, a list item from the paragraph
///   before it, and a fenced block and a heading from prose.
/// - witness: `doc_shape::tests::paragraphs_compare_without_wrapping_or_link_targets`
#[spec(ensures: |output| output.iter().all(|paragraph| paragraph.line < lines.len())
    && output.is_sorted_by_key(|paragraph| paragraph.line))]
fn paragraphs(lines: &[DocLine]) -> Vec<Paragraph>
{
    let mut found: Vec<Paragraph> = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.text.trim();
        let separates = line.code
            || trimmed.is_empty()
            || heading_level(RustdocLine::from(trimmed)).0 > 0
            || trimmed.starts_with('|')
            || trimmed.starts_with('<')
            || (trimmed.starts_with('[') && trimmed.contains("]:"));
        let item = list_item(RustdocLine::from(trimmed));
        if separates || matches!(item, Maybe::Present(_)) {
            if let Some((start, text)) = open.take() {
                found.push(Paragraph {
                    line: start,
                    words: paragraph_words(RustdocText::from(text.as_str())),
                });
            }
            if separates {
                continue;
            }
        }
        let text = match item {
            Maybe::Present(rest) => rest.0,
            Maybe::Absent(marker_check::Unmarked::NoLabel) => trimmed,
        };
        match open {
            Some((_, ref mut joined)) => {
                joined.push(' ');
                joined.push_str(text);
            },
            None => open = Some((index, text.to_owned())),
        }
    }
    if let Some((start, text)) = open {
        found.push(Paragraph {
            line: start,
            words: paragraph_words(RustdocText::from(text.as_str())),
        });
    }
    found
}

/// The text after a list item's marker.
///
/// # Specification
/// - ensures: returns the text after `- `, `* `, or a run of digits and `. `
///   when the trimmed line opens with one.
/// - provides: `marker_check::Unmarked::NoLabel` for a line that opens no list
///   item.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — as [`paragraphs`].
/// - witness: `doc_shape::tests::paragraphs_compare_without_wrapping_or_link_targets`
#[spec(ensures: |output| !matches!(output, Maybe::Present(rest) if rest.0.len() >= line.0.len()))]
fn list_item(line: RustdocLine<'_>) -> Maybe<RustdocLine<'_>, marker_check::Unmarked>
{
    if let Some(rest) = line
        .0
        .strip_prefix("- ")
        .or_else(|| line.0.strip_prefix("* "))
    {
        return Maybe::Present(RustdocLine::from(rest));
    }
    let digits = line
        .0
        .trim_start_matches(|character: char| character.is_ascii_digit());
    if digits.len() < line.0.len()
        && let Some(rest) = digits.strip_prefix(". ")
    {
        return Maybe::Present(RustdocLine::from(rest));
    }
    Maybe::Absent(marker_check::Unmarked::NoLabel)
}

/// A paragraph's compared words.
///
/// # Specification
/// - ensures: returns the whitespace-separated words of `text` once every `(…)`
///   or `[…]` directly after a `]` is removed and every `*`, backtick, `[` and
///   `]` is dropped.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — as [`paragraphs`].
/// - witness: `doc_shape::tests::paragraphs_compare_without_wrapping_or_link_targets`
#[spec(ensures: |output| output.iter().all(|word| !word.is_empty() && !word.contains(['*', '`', '[', ']'])))]
fn paragraph_words(text: RustdocText<'_>) -> Vec<String>
{
    let mut kept = String::new();
    let mut characters = text.0.chars().peekable();
    while let Some(character) = characters.next() {
        if character == ']'
            && let Some(&opening @ ('(' | '[')) = characters.peek()
        {
            let closing = if opening == '(' { ')' } else { ']' };
            for skipped in characters.by_ref() {
                if skipped == closing {
                    break;
                }
            }
            continue;
        }
        if !matches!(character, '*' | '`' | '[' | ']') {
            kept.push(character);
        }
    }
    kept.split_whitespace().map(str::to_owned).collect()
}

#[cfg(test)]
mod tests
{
    use quenchant_shape::shape::Maybe;

    use super::CRATE_ROOT_BUDGET;
    use super::DecisionDefect;
    use super::DocLine;
    use super::HeadingDefect;
    use super::HeadingState;
    use super::MODULE_BUDGET;
    use super::Part;
    use super::budget_judgement;
    use super::classify;
    use super::doc_bytes;
    use super::heading_judgement;
    use super::heading_level;
    use super::history_findings;
    use super::paragraphs;
    use super::read_decisions;
    use super::row_judgement;
    use crate::semantic::CrateRoot;
    use crate::semantic::DocBytes;
    use crate::semantic::RustdocLine;
    use crate::semantic::RustdocText;

    /// Fixture lines, read as the module reads documentation.
    ///
    /// # Specification
    /// trivial.
    fn doc(text: RustdocText<'_>) -> Vec<DocLine>
    {
        classify(text)
    }

    /// The phrases a fixture's history reading finds.
    ///
    /// # Specification
    /// trivial.
    fn phrases(text: RustdocText<'_>) -> Vec<String>
    {
        history_findings(&doc(text))
            .into_iter()
            .map(|finding| finding.phrase.to_owned())
            .collect()
    }

    #[test]
    fn a_fenced_line_is_neither_heading_nor_prose()
    {
        let lines =
            doc(" Lead.\n ```rust,ignore\n # hidden\n ```\n # Items\n `` ```` `` spans.".into());
        let code: Vec<bool> = lines.iter().map(|line| line.code).collect();
        assert_eq!(
            code,
            [false, true, true, true, false, false],
            "the fences and what they hold are code; the prose after the close is not"
        );
        assert_eq!(
            heading_level(RustdocLine(" # Items")).0,
            1_usize,
            "a `#` heading"
        );
        assert_eq!(
            heading_level(RustdocLine("## Sub")).0,
            2_usize,
            "a `##` heading"
        );
        assert_eq!(
            heading_level(RustdocLine("#Items")).0,
            0_usize,
            "a run without its space"
        );
        let mut state = HeadingState::default();
        let refused: Vec<_> = lines
            .iter()
            .filter(|line| matches!(heading_judgement(line, &mut state), Maybe::Present(_)))
            .collect();
        assert!(
            refused.is_empty(),
            "the fenced `# hidden` is no heading: {refused:?}"
        );
    }

    #[test]
    fn headings_are_judged_against_the_set()
    {
        let lines = doc(concat!(
            " # Decisions\n",
            " ## Sub\n",
            " # Status\n",
            " ## Detail\n",
            " # Specification\n",
            " # Decisions\n"
        )
        .into());
        let mut state = HeadingState::default();
        let judged: Vec<_> = lines
            .iter()
            .map(|line| heading_judgement(line, &mut state))
            .collect();
        assert_eq!(
            judged,
            [
                Maybe::Absent(super::heading_check::Admitted::ModuleHeading),
                Maybe::Absent(super::heading_check::Admitted::UnderDecisions),
                Maybe::Present(HeadingDefect::OutsideSet("# Status".to_owned())),
                Maybe::Present(HeadingDefect::OutsideSet("## Detail".to_owned())),
                Maybe::Absent(super::heading_check::Admitted::EvidenceHeading),
                Maybe::Present(HeadingDefect::Repeated("# Decisions".to_owned())),
                Maybe::Absent(super::heading_check::Admitted::NotAHeading),
            ],
            "a sub-heading is the decision rule's only under `# Decisions`"
        );
    }

    #[test]
    fn a_row_is_three_labelled_bullets_in_order()
    {
        let reading = read_decisions(&doc(concat!(
            " # Decisions\n",
            " - Choice: one.\n",
            " - Alternative: the singular.\n",
            " - Reversal:\n",
            "   wrapped value.\n",
            " - Choice: two.\n",
            " - Alternatives: a.\n",
            " - Reversal: b.\n",
            " - Alternatives: a fourth bullet.\n",
            " - Choice: three.\n",
            " - Reversal: before the alternatives.\n",
            " - Alternatives: late.\n",
            " - **Bold.** an unlabelled bullet.\n",
            " - Choice: four.\n",
            " - Alternatives:\n",
            " - Reversal: c.\n",
            "\n",
            " A paragraph\n",
            " of two lines.\n",
            " ## Sub\n",
            " # Items\n",
            " - Choice: past the section.\n"
        )
        .into()));
        let judged: Vec<_> = reading.inside.iter().map(row_judgement).collect();
        assert_eq!(
            judged,
            [
                Maybe::Absent(super::row_check::Accepted::Row),
                Maybe::Present(DecisionDefect::NotOneRow(vec![
                    Part::Choice,
                    Part::Alternatives,
                    Part::Reversal,
                    Part::Alternatives,
                ])),
                Maybe::Present(DecisionDefect::NotOneRow(vec![
                    Part::Choice,
                    Part::Reversal,
                    Part::Alternatives,
                ])),
                Maybe::Present(DecisionDefect::Unlabelled),
                Maybe::Present(DecisionDefect::EmptyValue(Part::Alternatives)),
                Maybe::Present(DecisionDefect::Prose),
                Maybe::Present(DecisionDefect::SubHeading),
            ],
            "each decision is judged once, a wrapped value stated and a two-line paragraph one"
        );
        assert_eq!(
            reading.outside,
            [21_usize],
            "a label past the next level-one heading is outside the section"
        );
    }

    #[test]
    fn decisions_outside_the_section_are_counted()
    {
        let reading = read_decisions(&doc(concat!(
            " **Choice.** one.\n",
            " **Alternatives.** a.\n",
            " **Reversal.** b.\n",
            " **A bold lead.** prose.\n",
            "\n",
            " Reversal: without its choice.\n",
            " - Choice: unclosed.\n",
            " - Choice: also unclosed.\n",
            " The Reversal: inside a sentence.\n",
            " Reversals are not labels.\n"
        )
        .into()));
        assert_eq!(
            reading.outside,
            [0_usize, 5, 6, 7],
            "a reversal closes the open choice, an unclosed choice stands alone"
        );
        assert!(
            reading.inside.is_empty(),
            "no section, no decision inside it"
        );
    }

    #[test]
    fn used_to_after_a_form_of_be_is_the_present()
    {
        assert_eq!(
            phrases(
                concat!(
                    " Previously, formerly and historically. No longer, as of now.\n",
                    " It used to parse. It is used to parse; it's used to parse.\n",
                    " The field was added in a release; it was checked in a test.\n",
                    " `previously` and [`formerly`] are code.\n",
                    " A key no longer than a word passes, as [`read`] of a node does."
                )
                .into()
            ),
            [
                "previously",
                "formerly",
                "historically",
                "no longer",
                "as of",
                "used to",
                "was added in",
            ],
            "the passive, the code spans and the comparison pass, and `was checked in` is no \
             change"
        );
    }

    #[test]
    fn a_phrase_wrapped_across_lines_is_found()
    {
        let lines = doc(" The reader checks no\n longer what it must.\n ```\n as of\n ```".into());
        let findings = history_findings(&lines);
        assert_eq!(
            findings.len(),
            1_usize,
            "the fenced phrase is code: {findings:?}"
        );
        assert_eq!(
            findings
                .first()
                .map(|finding| (finding.line, finding.phrase)),
            Some((0_usize, "no longer")),
            "a phrase wrapped across lines is found at its first word"
        );
    }

    #[test]
    fn the_budget_admits_its_cap_and_refuses_one_byte_more()
    {
        assert_eq!(
            doc_bytes(&doc(" é\n".into())).0,
            5_usize,
            "three bytes of text, an empty line, one break each"
        );
        for (cap, root) in [
            (MODULE_BUDGET, CrateRoot(false)),
            (CRATE_ROOT_BUDGET, CrateRoot(true)),
        ] {
            assert!(
                matches!(budget_judgement(DocBytes(cap), root), Maybe::Absent(_)),
                "the cap itself is admitted"
            );
            assert_eq!(
                budget_judgement(DocBytes(cap.saturating_add(1)), root),
                Maybe::Present(DocBytes(cap.saturating_add(1))),
                "and one byte more is refused"
            );
        }
        assert!(
            matches!(
                budget_judgement(DocBytes(MODULE_BUDGET.saturating_add(1)), CrateRoot(true)),
                Maybe::Absent(_)
            ),
            "a crate root carries the larger budget"
        );
    }

    #[test]
    fn paragraphs_compare_without_wrapping_or_link_targets()
    {
        let rustdoc = paragraphs(&doc(concat!(
            " The reader *keeps* a [`Frame`]\n",
            " per [connection](crate::net) and a [table][t].\n",
            " - an item\n",
            "   wrapped.\n",
            " ```\n",
            " code\n",
            " ```\n",
            " # Heading\n",
            " [t]: crate::table\n"
        )
        .into()));
        let readme = paragraphs(&doc(
            "The reader keeps a [`Frame`](src/frame.rs) per connection and a table.\n\n1. an item wrapped.\n| a | b |"
                .into(),
        ));
        let words = |found: &[super::Paragraph]| {
            found
                .iter()
                .map(|paragraph| paragraph.words.join(" "))
                .collect::<Vec<String>>()
        };
        assert_eq!(
            words(&rustdoc),
            [
                "The reader keeps a Frame per connection and a table.",
                "an item wrapped."
            ],
            "a fence, a heading and a link definition are no paragraph"
        );
        assert_eq!(
            words(&rustdoc),
            words(&readme),
            "and the two texts carry the same paragraphs"
        );
    }
}
