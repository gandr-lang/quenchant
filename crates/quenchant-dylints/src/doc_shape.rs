//! Module documentation keeps a shape a reader scans and a reviewer prunes: a
//! lead paragraph, then sections from one vocabulary, each within a budget,
//! stating what the code does and nothing of how it came to be.
//!
//! The `rustdoc_shape` group gathers the refusals: [`MODULE_DOC_HEADING`],
//! [`DOC_DECISION_RECORD`], [`DOC_ISSUE_REFERENCE`], [`MODULE_DOC_BUDGET`] and
//! [`MODULE_DOC_DUPLICATION`]. [`DOC_HISTORY_PROSE`] stands outside it, a
//! report a lane forces to `warn`, and [`MODULE_DOC_MEASURE`] prints each
//! module's sizes. A test compilation is not read: a regression test names the
//! defect it pins.
//!
//! # Items
//!
//! - [`SECTIONS`], [`EVIDENCE_HEADINGS`], [`FOLD_INS`] and
//!   [`REFUSED_HEADINGS`]: the heading vocabulary.
//! - [`LEAD_BUDGET`], [`SECTION_BUDGET`], [`CRATE_ROOT_BUDGET`] and
//!   [`MODULE_BUDGET`]: the byte budgets, measured over the documentation's
//!   text with its comment markers stripped and one byte per line break.
//! - [`DUPLICATE_WORDS`]: the length at which a repeated paragraph is refused.
//!
//! # Boundaries
//!
//! Fenced code is code: a `#` line inside a fence opens no heading, and a
//! phrase, a label or an issue number inside a fence or a code span is not
//! read. The rules read words, not meaning: a decision written without its
//! labels and a past narrated without the listed phrases pass.

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

use crate::semantic::CrateRoot;
use crate::semantic::DecisionTable;
use crate::semantic::DocBytes;
use crate::semantic::HeadingLevel;
use crate::semantic::HistoryPhrase;
use crate::semantic::LineIndex;
use crate::semantic::OpensFence;
use crate::semantic::OuterDocumentation;
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
            /// The heading is a section of the vocabulary, opened once.
            Section,
            /// The heading is a fold-in spelling of a section, opened once.
            FoldIn,
            /// The heading opens an evidence section an item reader owns.
            Evidence,
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
        /// Why a measured span of documentation raises no budget finding.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Within {
            /// The span is at or under its cap.
            UnderBudget,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A line can carry no list marker or decision label.
    mod marker_check {
        /// Why a line or a sentence carries no marker.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unmarked {
            /// The text opens with none of the markers looked for.
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

/// The bytes a module's lead, the text before its first heading, may carry.
pub const LEAD_BUDGET: usize = 1_536;

/// The bytes one section, from its heading to the next, may carry.
pub const SECTION_BUDGET: usize = 2_048;

/// The bytes of documentation a crate root may carry.
pub const CRATE_ROOT_BUDGET: usize = 16_usize * 1_024_usize;

/// The bytes of documentation any other module may carry.
pub const MODULE_BUDGET: usize = 8_usize * 1_024_usize;

/// The words a paragraph carries before its repetition is refused.
pub const DUPLICATE_WORDS: usize = 25;

/// The lint group the shape refusals share.
pub const GROUP: &str = "rustdoc_shape";

/// Sections a module's documentation may open, each once.
const SECTIONS: [&str; 6] = [
    "Examples",
    "Witnesses",
    "Boundaries",
    "Items",
    "Dependencies",
    "References",
];

/// Evidence sections the item readers own, admitted in a module too.
const EVIDENCE_HEADINGS: [&str; 5] = ["Specification", "Adequacy", "Errors", "Panics", "Safety"];

/// Spellings read as a section of the vocabulary: the spelling, then the
/// section.
const FOLD_INS: [(&str, &str); 8] = [
    ("Example", "Examples"),
    ("Specification attributes", "Witnesses"),
    ("Specification evidence", "Witnesses"),
    ("Specification discipline", "Witnesses"),
    ("Specification and adequacy", "Witnesses"),
    ("Specification and evidence", "Witnesses"),
    ("Exit codes", "Boundaries"),
    ("Boundary wrappers", "Boundaries"),
];

/// Headings refused by name, each with where its content belongs.
const REFUSED_HEADINGS: [(&str, &str); 8] = [
    ("History", "history lives in version control"),
    ("Status", "status lives in the tracker"),
    (
        "Notes",
        "a note belongs in the section it qualifies, or in the lead",
    ),
    (
        "Changelog",
        "the changelog is written from the commits when a release is cut",
    ),
    (
        "Background",
        "background belongs in the references the module cites",
    ),
    ("Overview", "the lead paragraph is the overview"),
    ("Decisions", DECISION_HOME),
    ("Decision", DECISION_HOME),
];

/// Where a decision lives instead of rustdoc.
const DECISION_HOME: &str =
    "a decision lives in the pull request and the issue, where Git dates it";

/// Help attached to every heading finding.
const VOCABULARY_HELP: &str = concat!(
    "a module's documentation is a lead paragraph, then sections from `# Examples`, ",
    "`# Witnesses`, `# Boundaries`, `# Items`, `# Dependencies` and `# References`, beside the ",
    "evidence sections `# Specification`, `# Adequacy`, `# Errors`, `# Panics` and `# Safety`; ",
    "fold the section into one of them, into the lead, or onto the item it describes",
);

/// Help attached to every decision finding.
const DECISION_HELP: &str = concat!(
    "state the decision, its alternatives and its reversal in the pull request body and the ",
    "issue, where Git dates them; the documentation states what the code does",
);

/// A form of *be* or *get* before `used to` makes it the present passive.
const PASSIVE_AUXILIARIES: [&str; 12] = [
    "is", "are", "was", "were", "be", "been", "being", "s", "get", "gets", "got", "getting",
];

/// Words that narrate a past on their own.
const HISTORY_WORDS: [&str; 3] = ["previously", "formerly", "historically"];

declare_lint! {
    /// ### What it does
    ///
    /// Reads every module's own `//!` documentation and reports each heading outside its vocabulary. The text before the first heading is the lead paragraph; the sections are `# Examples`, `# Witnesses`, `# Boundaries`, `# Items`, `# Dependencies` and `# References`, each at most once, beside the evidence sections `# Specification`, `# Adequacy`, `# Errors`, `# Panics` and `# Safety`. Eight spellings fold into a section and count as it ([`FOLD_INS`]): `# Example`; `# Specification attributes`, `evidence`, `discipline`, `and adequacy` and `and evidence`; `# Exit codes` and `# Boundary wrappers`. `# History`, `# Status`, `# Notes`, `# Changelog`, `# Background`, `# Overview`, `# Decisions` and `# Decision` are refused by name, each with where its content belongs ([`REFUSED_HEADINGS`]). Any other heading is outside the vocabulary, and a heading below `#` is a sub-section: module documentation has one level of sections.
    ///
    /// ### Why is this bad?
    ///
    /// A reader scans a module's documentation by its sections. Topic headings make every module a different document; a status, a history or a decision record narrates instead of stating, and goes stale where it stands.
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
    "module documentation opens only the sections of its vocabulary"
}

declare_lint! {
    /// ### What it does
    ///
    /// Reports a decision record in module and item documentation alike: a `Choice`, `Alternative`, `Alternatives` or `Reversal` label followed by `:` or `.` opening a line, a bullet or a sentence, bold or plain; a table row with a `Choice` cell and a `Reversal` cell; and, in item documentation, a `# Decisions` or `# Decision` heading ([`MODULE_DOC_HEADING`] refuses it in a module's). Labels read in order form one record: a `Choice` opens one, a `Reversal` closes it, and each record is reported once at its first label.
    ///
    /// ### Why is this bad?
    ///
    /// Documentation states what the code does. A decision's alternatives and reversal accumulate, outlive the choice and keep the reasons for paths already abandoned; the pull request and the issue hold them with their date.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// //! The cache is per process.
    /// //!
    /// //! - Choice: one cache per process.
    /// //! - Alternatives: a shared cache, whose invalidation costs more.
    /// //! - Reversal: a rebuild slower than the invalidation.
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// //! The cache is per process: a rebuild costs less than invalidating a
    /// //! shared one.
    /// ```
    pub DOC_DECISION_RECORD,
    Allow,
    "documentation states what the code does, not the decisions that led to it"
}

declare_lint! {
    /// ### What it does
    ///
    /// Reports an issue reference — `#` followed by digits, alone or after an `owner/repository` — in module and item documentation. Fenced code, code spans and HTML character references are not read, and neither is a test compilation: a regression test names the defect it pins.
    ///
    /// ### Why is this bad?
    ///
    /// An issue number in documentation points at a design's origin instead of stating the design. The reader follows it into a discussion that has moved on; the pull request and the commit carry the reference with their date.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// /// Reads the header first (#577).
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// /// Reads the header first, because the frame length lives there.
    /// ```
    pub DOC_ISSUE_REFERENCE,
    Allow,
    "documentation states its reasons instead of citing an issue"
}

declare_lint! {
    /// ### What it does
    ///
    /// Measures each module's `//!` documentation: the bytes of its text with one byte per line break. A crate root may carry 16384 bytes ([`CRATE_ROOT_BUDGET`]) and any other module 8192 ([`MODULE_BUDGET`]); the lead, the text before the first heading, 1536 ([`LEAD_BUDGET`]); and each section, from its heading to the next, 2048 ([`SECTION_BUDGET`]). The whole and the lead are reported at the documentation's first line, a section at its heading.
    ///
    /// ### Why is this bad?
    ///
    /// A README's budget moves detail into rustdoc, where nothing prunes it. A budget by structure makes growth a decision: the module splits, or a section moves to the item it describes.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and belongs to the `rustdoc_shape` group.
    pub MODULE_DOC_BUDGET,
    Allow,
    "a module's documentation, its lead and each section fit their byte budgets"
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
    /// Reports the words and phrases that narrate a past instead of stating the present — `previously`, `formerly`, `historically`, `no longer`, `as of`, `used to` and `was added`, `removed`, `renamed` or `changed` `in` — in module and item documentation alike. `used to` after a form of *be* or *get* is the present passive and passes, and so does the comparison `no longer than`. Fenced code and code spans are not read.
    ///
    /// ### Why is this bad?
    ///
    /// Documentation states what is. A superseded statement is replaced, never annotated; history lives in version control. The phrases also state the present — a postcondition, a runtime order — so the lint reports and a reader judges.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and outside the `rustdoc_shape` group: a lane selects it with `--force-warn`, as a report that denied warnings never turn into a refusal.
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
    /// Prints, at the first line of each module's `//!` documentation, its bytes as [`MODULE_DOC_BUDGET`] measures them, its lead's bytes, its largest section's bytes and the decision records [`DOC_DECISION_RECORD`] reads in it.
    ///
    /// ### Why is this bad?
    ///
    /// Nothing is wrong with a measured module. The measure is the distribution a report reads, selected alone over a consumer.
    ///
    /// ### Activation
    ///
    /// The lint is `allow` by default and outside the `rustdoc_shape` group.
    pub MODULE_DOC_MEASURE,
    Allow,
    "print each module's documentation sizes and decision records"
}

impl_lint_pass!(DocShape => [
    MODULE_DOC_HEADING,
    DOC_DECISION_RECORD,
    DOC_ISSUE_REFERENCE,
    MODULE_DOC_BUDGET,
    MODULE_DOC_DUPLICATION,
    DOC_HISTORY_PROSE,
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
    /// The heading is outside the vocabulary.
    OutsideVocabulary(String),
    /// The heading is refused by name, with where its content belongs.
    Refused(String, &'static str),
    /// A section, under its own spelling or a fold-in, opens a second time.
    Repeated(&'static str),
    /// The heading sits below `#`.
    SubSection(String),
}

/// One decision label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Part
{
    /// `Choice`.
    Choice,
    /// `Alternatives` or `Alternative`.
    Alternatives,
    /// `Reversal`.
    Reversal,
}

/// The form a decision record takes.
#[derive(Clone, Debug, Eq, PartialEq)]
enum RecordKind
{
    /// Labelled lines, bullets or sentences, in the order read.
    Labelled(Vec<Part>),
    /// A table row naming a choice and a reversal.
    Table,
    /// A decision heading in item documentation.
    Heading(String),
}

/// One decision record and the line it opens on.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DecisionRecord
{
    /// The line of the record's first label, row or heading.
    line: usize,
    /// The record's form.
    kind: RecordKind,
}

/// One issue reference and the line it stands on.
#[derive(Clone, Debug, Eq, PartialEq)]
struct IssueReference
{
    /// The line carrying the reference.
    line: usize,
    /// The reference as written, its repository included.
    text: String,
}

/// The headings a module's documentation has opened, carried line to line.
#[derive(Default)]
#[repr(transparent)]
struct HeadingState
{
    /// The sections opened so far, each under its vocabulary spelling.
    opened: Vec<&'static str>,
}

/// One section of a module's documentation and its size.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SectionSize
{
    /// The line of the section's heading.
    line: usize,
    /// The heading as written.
    heading: String,
    /// The section's bytes, its heading line included.
    bytes: DocBytes,
}

/// A module's documentation measured by its structure.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Structure
{
    /// The bytes before the first heading.
    lead: DocBytes,
    /// Each section, in order.
    sections: Vec<SectionSize>,
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
    /// Read every node's documentation for decision records, issue references
    /// and history prose.
    ///
    /// # Specification
    /// - ensures: outside a test compilation, reports at the attribute carrying
    ///   it each record [`decision_records`] reads, each reference
    ///   [`issue_references`] reads and each phrase [`history_findings`] finds,
    ///   in the node's outer documentation and, separately, its inner
    ///   documentation; a decision heading is read in the outer documentation
    ///   alone, and documentation an external macro wrote is not read.
    /// - panics: none.
    /// - executable: none — rustc owns the diagnostic sink; this callback has
    ///   no readback of emitted or suppressed diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the decision, issue and history fixtures carry each
    ///   form in item and module documentation, beside code spans, fences and a
    ///   decision heading in a module, none of those three reported; the
    ///   test-compilation fixture carries every form and reports none.
    /// - witness: `tests::ui_doc_shape`
    fn check_attributes(
        &mut self,
        cx: &LateContext<'tcx>,
        attrs: &'tcx [Attribute],
    )
    {
        if cx.tcx.sess.is_test_crate() {
            return;
        }
        for style in [AttrStyle::Outer, AttrStyle::Inner] {
            let text = doc_text(cx, attrs, style);
            if text.lines.is_empty() {
                continue;
            }
            let outer = OuterDocumentation(matches!(style, AttrStyle::Outer));
            for record in decision_records(&text.lines, outer) {
                let Some(&span) = text.spans.get(record.line)
                else {
                    continue;
                };
                let message = match record.kind {
                    RecordKind::Labelled(parts) => format!(
                        "this decision record reads {}; documentation states what the code does",
                        part_names(&parts)
                    ),
                    RecordKind::Table => {
                        "this table records decisions; documentation states what the code does"
                            .to_owned()
                    },
                    RecordKind::Heading(heading) => {
                        format!("`{heading}` opens a decision record in item documentation")
                    },
                };
                span_lint_and_help(cx, DOC_DECISION_RECORD, span, message, None, DECISION_HELP);
            }
            for reference in issue_references(&text.lines) {
                let Some(&span) = text.spans.get(reference.line)
                else {
                    continue;
                };
                span_lint_and_help(
                    cx,
                    DOC_ISSUE_REFERENCE,
                    span,
                    format!(
                        "`{}` cites an issue; documentation states its reasons without one",
                        reference.text
                    ),
                    None,
                    "state the reason here, and cite the issue in the pull request or the commit",
                );
            }
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
    /// - ensures: outside a test compilation, for the crate root and each
    ///   module, reads its inner documentation and reports, at the module, each
    ///   heading [`heading_judgement`] refuses, documentation, a lead or a
    ///   section over its budget, and each paragraph of [`DUPLICATE_WORDS`]
    ///   words or more the README or an earlier module carries; then prints
    ///   [`MODULE_DOC_MEASURE`] for every documented module.
    /// - panics: none.
    /// - executable: none — rustc owns the diagnostic sink; this callback has
    ///   no readback of emitted or suppressed diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — one fixture per refusal carries the refused shapes at
    ///   a crate root and in modules, and the accepted fixture carries every
    ///   section, fold-ins and evidence headings and a repeated short
    ///   paragraph, none reported.
    /// - witness: `tests::ui_doc_shape`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        if cx.tcx.sess.is_test_crate() {
            return;
        }
        let mut seen: BTreeMap<Vec<String>, FirstSeen> = BTreeMap::new();
        cx.tcx.hir_for_each_module(|module| {
            let def_id = module.to_local_def_id();
            let hir_id = cx.tcx.local_def_id_to_hir_id(def_id);
            let root = CrateRoot(hir_id == CRATE_HIR_ID);
            let text = doc_text(cx, cx.tcx.hir_attrs(hir_id), AttrStyle::Inner);
            let Some(&first) = text.spans.first()
            else {
                return;
            };
            report_headings(cx, hir_id, &text, first);
            let bytes = doc_bytes(&text.lines);
            let structure = structure(&text.lines);
            report_budgets(cx, hir_id, &text, first, (bytes, root), &structure);
            self.report_duplicates(cx, hir_id, &text, &mut seen);
            let largest = structure
                .sections
                .iter()
                .map(|section| section.bytes.0)
                .max()
                .unwrap_or(0_usize);
            let records = decision_records(&text.lines, OuterDocumentation(false)).len();
            span_lint_hir(
                cx,
                MODULE_DOC_MEASURE,
                hir_id,
                first,
                format!(
                    "{} documentation: {} bytes, lead {} bytes, largest section {} bytes, {} \
                     decision records",
                    if root.0 { "crate root" } else { "module" },
                    bytes.0,
                    structure.lead.0,
                    largest,
                    records
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
/// - hypothesis: L3 — the heading fixture carries each refused name it reports,
///   a heading outside the vocabulary, a sub-section, a section repeated
///   through a fold-in, and a `#` line inside a fence, the last not reported.
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
        let (message, help) = match defect {
            HeadingDefect::OutsideVocabulary(heading) => (
                format!("`{heading}` is outside the module documentation vocabulary"),
                VOCABULARY_HELP,
            ),
            HeadingDefect::Refused(heading, home) => (
                format!("`{heading}` is refused in module documentation"),
                home,
            ),
            HeadingDefect::Repeated(section) => (
                format!("`# {section}` opens twice in this module's documentation"),
                VOCABULARY_HELP,
            ),
            HeadingDefect::SubSection(heading) => (
                format!("`{heading}` is a sub-section; module documentation has one level"),
                VOCABULARY_HELP,
            ),
        };
        span_lint_hir_and_then(cx, MODULE_DOC_HEADING, hir_id, span, message, |diag| {
            diag.help(help);
        });
    }
}

/// Report a module's documentation, lead and sections over their budgets.
///
/// # Specification
/// - requires: `text` is the inner documentation of the module `hir_id`,
///   `first` the span of its first line, `measured` its [`doc_bytes`] and
///   whether it is the crate root, and `structure` its [`structure`].
/// - ensures: reports [`MODULE_DOC_BUDGET`] at the first line when
///   [`budget_judgement`] refuses the whole or the lead, and at each section's
///   heading when it refuses the section.
/// - panics: none.
/// - executable: none — rustc owns the diagnostic sink; this function has no
///   readback of emitted or suppressed diagnostics.
///
/// # Adequacy
/// - hypothesis: L3 — the budget fixture carries a crate root over its cap with
///   every section under its own, and a module over its cap with a lead and one
///   section over theirs, each reported once.
/// - witness: `tests::ui_doc_shape`
fn report_budgets(
    cx: &LateContext<'_>,
    hir_id: HirId,
    text: &DocText,
    first: Span,
    measured: (DocBytes, CrateRoot),
    structure: &Structure,
)
{
    let (bytes, root) = measured;
    let (place, cap) = if root.0 {
        ("crate root", CRATE_ROOT_BUDGET)
    }
    else {
        ("module", MODULE_BUDGET)
    };
    let report = |span: Span, message: String, help: &'static str| {
        span_lint_hir_and_then(cx, MODULE_DOC_BUDGET, hir_id, span, message, |diag| {
            diag.help(help);
        });
    };
    if let Maybe::Present(excess) = budget_judgement(bytes, DocBytes(cap)) {
        report(
            first,
            format!(
                "this {place}'s documentation is {} bytes, over its {cap}-byte budget",
                excess.0
            ),
            "split the module, or move a section to the item it describes; never compress the \
             prose to fit",
        );
    }
    if let Maybe::Present(excess) = budget_judgement(structure.lead, DocBytes(LEAD_BUDGET)) {
        report(
            first,
            format!(
                "this {place}'s lead is {} bytes, over its {LEAD_BUDGET}-byte budget",
                excess.0
            ),
            "keep the lead to what the module is; move the detail into a section or onto the \
             item it describes",
        );
    }
    for section in &structure.sections {
        if let Maybe::Present(excess) = budget_judgement(section.bytes, DocBytes(SECTION_BUDGET)) {
            report(
                text.spans.get(section.line).copied().unwrap_or(first),
                format!(
                    "the section `{}` is {} bytes, over its {SECTION_BUDGET}-byte budget",
                    section.heading, excess.0
                ),
                "move the detail onto the items the section describes, or split the module",
            );
        }
    }
}

/// Name a run of decision labels.
///
/// # Specification
/// trivial.
fn part_names(parts: &[Part]) -> String
{
    parts
        .iter()
        .map(|part| match *part {
            Part::Choice => "`Choice`",
            Part::Alternatives => "`Alternatives`",
            Part::Reversal => "`Reversal`",
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
///   on one module and an empty line from a paragraph's last.
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

/// The title a heading line carries, its `#` run and spaces removed.
///
/// # Specification
/// - ensures: returns the trimmed line without its leading `#` run, trimmed
///   again.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests read titles at two levels and with trailing
///   spaces.
/// - witness: `doc_shape::tests::headings_are_judged_against_the_vocabulary`
#[spec(ensures: |output| line.0.contains(output.0))]
fn heading_title(line: RustdocLine<'_>) -> RustdocLine<'_>
{
    RustdocLine::from(line.0.trim().trim_start_matches('#').trim())
}

/// Judge one line of a module's documentation as a heading.
///
/// # Specification
/// - requires: the lines of one module are judged in order, with `state`
///   carried from line to line and starting empty.
/// - ensures: admits a line that opens no heading or sits in fenced code, an
///   evidence heading, and a section of [`SECTIONS`] or a spelling of
///   [`FOLD_INS`] whose section `state` has not opened; refuses a heading below
///   `#`, then a title of [`REFUSED_HEADINGS`] with its reason, then a section
///   opened again, then any other heading as outside the vocabulary.
/// - provides: `heading_check::Admitted` names why the line is admitted.
/// - panics: none.
/// - executable: none — the judgement threads mutable state across calls; the
///   tests and the UI fixtures are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate every admitted reason and every
///   refusal, a fold-in repeating its section among them.
/// - witness: `doc_shape::tests::headings_are_judged_against_the_vocabulary`
fn heading_judgement(
    line: &DocLine,
    state: &mut HeadingState,
) -> Maybe<HeadingDefect, heading_check::Admitted>
{
    let level = heading_level(RustdocLine::from(&line.text)).0;
    if line.code || level == 0 {
        return Maybe::Absent(heading_check::Admitted::NotAHeading);
    }
    let heading = line.text.trim();
    if level > 1 {
        return Maybe::Present(HeadingDefect::SubSection(heading.to_owned()));
    }
    let title = heading_title(RustdocLine::from(heading)).0;
    if let Some(refused) = REFUSED_HEADINGS.iter().find(|entry| entry.0 == title) {
        return Maybe::Present(HeadingDefect::Refused(heading.to_owned(), refused.1));
    }
    let (section, admitted) = if let Some(fold_in) = FOLD_INS.iter().find(|entry| entry.0 == title)
    {
        (fold_in.1, heading_check::Admitted::FoldIn)
    }
    else if let Some(&section) = SECTIONS.iter().find(|&&section| section == title) {
        (section, heading_check::Admitted::Section)
    }
    else if EVIDENCE_HEADINGS.contains(&title) {
        return Maybe::Absent(heading_check::Admitted::Evidence);
    }
    else {
        return Maybe::Present(HeadingDefect::OutsideVocabulary(heading.to_owned()));
    };
    if state.opened.contains(&section) {
        return Maybe::Present(HeadingDefect::Repeated(section));
    }
    state.opened.push(section);
    Maybe::Absent(admitted)
}

/// The decision records a node's documentation carries.
///
/// # Specification
/// - ensures: returns, ordered by line, one record per table row
///   [`decision_table`] reads; when `outer` holds, one per heading titled
///   `Decisions` or `Decision`; and one per run of [`decision_labels`] read in
///   order, where a `Choice` closes the open run and opens one, an
///   `Alternatives` joins the open run or opens one, a `Reversal` joins the
///   open run or opens one and closes it, and a heading or a table row closes
///   the open run. Fenced lines are not read.
/// - panics: none.
/// - executable: none — the reading is a fold over the lines whose statement
///   restates it; the tests and the UI fixtures are the oracle.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a three-bullet record, a bold-lead
///   paragraph with inline labels, a record without its choice, two choices in
///   a row, a table row, a decision heading in outer and inner documentation,
///   and labels in a fence and a code span.
/// - witness: `doc_shape::tests::labelled_runs_are_decision_records`
fn decision_records(
    lines: &[DocLine],
    outer: OuterDocumentation,
) -> Vec<DecisionRecord>
{
    let mut records = Vec::new();
    let mut open: Option<(usize, Vec<Part>)> = None;
    let close = |run: Option<(usize, Vec<Part>)>| {
        run.map(|(line, parts)| DecisionRecord {
            line,
            kind: RecordKind::Labelled(parts),
        })
    };
    for (index, line) in lines.iter().enumerate() {
        if line.code {
            continue;
        }
        let text = RustdocLine::from(&line.text);
        if heading_level(text).0 > 0 {
            records.extend(close(open.take()));
            let title = heading_title(text).0;
            if outer.0 && (title == "Decisions" || title == "Decision") {
                records.push(DecisionRecord {
                    line: index,
                    kind: RecordKind::Heading(line.text.trim().to_owned()),
                });
            }
            continue;
        }
        if decision_table(text).0 {
            records.extend(close(open.take()));
            records.push(DecisionRecord {
                line: index,
                kind: RecordKind::Table,
            });
            continue;
        }
        for part in decision_labels(text) {
            match part {
                Part::Choice => {
                    records.extend(close(open.replace((index, vec![Part::Choice]))));
                },
                Part::Alternatives => match open {
                    Some((_, ref mut parts)) => parts.push(Part::Alternatives),
                    None => open = Some((index, vec![Part::Alternatives])),
                },
                Part::Reversal => {
                    let (start, mut parts) = open.take().unwrap_or((index, Vec::new()));
                    parts.push(Part::Reversal);
                    records.extend(close(Some((start, parts))));
                },
            }
        }
    }
    records.extend(close(open));
    records.sort_by_key(|record| record.line);
    records
}

/// A table row names a choice and a reversal.
///
/// # Specification
/// - ensures: answers affirmatively exactly when the trimmed line opens with
///   `|` and, once emphasis and code marks are trimmed from each cell, one cell
///   is `Choice` and another opens with `Reversal`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a decision table's header from a row
///   naming one of the two and from a line that opens no table.
/// - witness: `doc_shape::tests::labelled_runs_are_decision_records`
#[spec(ensures: |output| !output.0 || line.0.trim_start().starts_with('|'))]
fn decision_table(line: RustdocLine<'_>) -> DecisionTable
{
    let trimmed = line.0.trim();
    if !trimmed.starts_with('|') {
        return DecisionTable(false);
    }
    let cells: Vec<&str> = trimmed
        .split('|')
        .map(|cell| cell.trim().trim_matches(['*', '_', '`']).trim())
        .collect();
    DecisionTable(
        cells.contains(&"Choice") && cells.iter().any(|cell| cell.starts_with("Reversal")),
    )
}

/// The decision labels opening a line's sentences, in order.
///
/// # Specification
/// - ensures: returns the label [`decision_label`] reads at each sentence
///   opening of the line once its code spans are masked: the line's start after
///   an optional list marker, a position after `(`, and a position after
///   whitespace that follows `.`, `!`, `?`, `;` or `:`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a bulleted, a bold and a plain label,
///   a label opening a later sentence and a parenthesis, a label inside a
///   sentence, a plural that is no label, and a label in a code span.
/// - witness: `doc_shape::tests::labelled_runs_are_decision_records`
#[spec(ensures: |output| output.is_empty()
    || ["Choice", "Alternative", "Reversal"].iter().any(|label| line.0.contains(label)))]
fn decision_labels(line: RustdocLine<'_>) -> Vec<Part>
{
    let masked = masked_code_spans(line);
    let trimmed = masked.trim();
    let body = match list_item(RustdocLine::from(trimmed)) {
        Maybe::Present(rest) => rest.0,
        Maybe::Absent(marker_check::Unmarked::NoLabel) => trimmed,
    };
    let mut labels = Vec::new();
    let mut boundary = false;
    let mut spaced = false;
    let mut previous = ' ';
    for (offset, character) in body.char_indices() {
        if character.is_whitespace() {
            spaced = true;
            previous = character;
            continue;
        }
        let opens = offset == 0 || (boundary && spaced) || previous == '(';
        if opens
            && let Some(rest) = body.get(offset ..)
            && let Maybe::Present(part) = decision_label(RustdocLine::from(rest))
        {
            labels.push(part);
        }
        boundary = matches!(character, '.' | '!' | '?' | ';' | ':');
        spaced = false;
        previous = character;
    }
    labels
}

/// The decision label a sentence opens with.
///
/// # Specification
/// - ensures: returns `Choice`, `Alternatives` or `Reversal` exactly when the
///   text, after optional `*` or `_` emphasis, opens with `Choice`,
///   `Alternative`, `Alternatives` or `Reversal`, then optional emphasis, then
///   `:` or `.`.
/// - provides: `marker_check::Unmarked::NoLabel` for every other text.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a plain and a bold label, the emphasis
///   closing before and after the colon, a label followed by a word, and a
///   plural.
/// - witness: `doc_shape::tests::labelled_runs_are_decision_records`
#[spec(ensures: |output| matches!(output, Maybe::Absent(_))
    || ["Choice", "Alternative", "Reversal"].iter().any(|label| text.0.contains(label)))]
fn decision_label(text: RustdocLine<'_>) -> Maybe<Part, marker_check::Unmarked>
{
    let plain = text.0.trim_start_matches(['*', '_']);
    for (label, part) in [
        ("Choice", Part::Choice),
        ("Alternatives", Part::Alternatives),
        ("Alternative", Part::Alternatives),
        ("Reversal", Part::Reversal),
    ] {
        if plain
            .strip_prefix(label)
            .is_some_and(|rest| rest.trim_start_matches(['*', '_']).starts_with([':', '.']))
        {
            return Maybe::Present(part);
        }
    }
    Maybe::Absent(marker_check::Unmarked::NoLabel)
}

/// A line with the text of its code spans masked.
///
/// # Specification
/// - ensures: returns the line with every character between a pair of backticks
///   replaced by `\u{1}`, the backticks kept; an unclosed backtick masks the
///   rest of the line.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests read a label and an issue number in a code
///   span, neither found.
/// - witness: `doc_shape::tests::labelled_runs_are_decision_records`
/// - witness: `doc_shape::tests::issue_numbers_in_prose_are_references`
#[spec(ensures: |output| output.chars().count() == line.0.chars().count())]
fn masked_code_spans(line: RustdocLine<'_>) -> String
{
    let mut in_span = false;
    line.0
        .chars()
        .map(|character| {
            if character == '`' {
                in_span = !in_span;
                return character;
            }
            if in_span { '\u{1}' } else { character }
        })
        .collect()
}

/// The issue references a node's documentation carries.
///
/// # Specification
/// - ensures: returns, in order, one reference per `#` outside fenced code and
///   code spans followed by one or more ASCII digits and then a character that
///   is neither alphanumeric nor `_`, when the character before the `#` is
///   absent, whitespace or punctuation other than `&`, or closes an
///   `owner/repository` run of alphanumerics, `-`, `_` and `.` around one `/`,
///   which the reference then includes.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate a bare and a parenthesised number, an
///   `owner/repository` reference, a number in a code span and a fence, an HTML
///   character reference, a number glued to a word, and a `#` followed by a
///   letter.
/// - witness: `doc_shape::tests::issue_numbers_in_prose_are_references`
#[spec(ensures: |output| output.iter().all(|reference| reference.line < lines.len()
    && reference.text.contains('#')))]
fn issue_references(lines: &[DocLine]) -> Vec<IssueReference>
{
    let mut references = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if line.code {
            continue;
        }
        let masked = masked_code_spans(RustdocLine::from(&line.text));
        for (offset, character) in masked.char_indices() {
            if character != '#' {
                continue;
            }
            let after = masked.get(offset.saturating_add(1) ..).unwrap_or("");
            let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
            let closed = after
                .chars()
                .nth(digits.chars().count())
                .is_none_or(|next| !next.is_alphanumeric() && next != '_');
            if digits.is_empty() || !closed {
                continue;
            }
            let before = masked.get(.. offset).unwrap_or("");
            if let Maybe::Present(repository) = repository_before(RustdocLine::from(before)) {
                references.push(IssueReference {
                    line: index,
                    text: format!("{}#{digits}", repository.0),
                });
            }
        }
    }
    references
}

/// The repository an issue number's `#` follows, empty for none.
///
/// # Specification
/// - ensures: returns the empty text when `before` is empty or ends in
///   whitespace or punctuation other than `&`, `-`, `_`, `.` and `/`; returns
///   the run of alphanumerics, `-`, `_`, `.` and `/` it ends in when that run
///   is two non-empty parts around one `/`.
/// - provides: `marker_check::Unmarked::NoLabel` when the `#` is glued to a
///   word, ends an HTML character reference's `&`, or follows a run that names
///   no repository.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — as [`issue_references`].
/// - witness: `doc_shape::tests::issue_numbers_in_prose_are_references`
#[spec(ensures: |output| !matches!(output, Maybe::Present(repository) if !before.0.ends_with(repository.0)))]
fn repository_before(before: RustdocLine<'_>) -> Maybe<RustdocLine<'_>, marker_check::Unmarked>
{
    let in_run = |character: char| character.is_alphanumeric() || "-_./".contains(character);
    let Some(last) = before.0.chars().next_back()
    else {
        return Maybe::Present(RustdocLine::from(""));
    };
    if last == '&' {
        return Maybe::Absent(marker_check::Unmarked::NoLabel);
    }
    if !in_run(last) {
        return Maybe::Present(RustdocLine::from(""));
    }
    let start = before
        .0
        .char_indices()
        .rev()
        .take_while(|&(_, character)| in_run(character))
        .last()
        .map_or(0_usize, |(offset, _)| offset);
    let run = before.0.get(start ..).unwrap_or("");
    match run.split_once('/') {
        Some((owner, repository))
            if !owner.is_empty() && !repository.is_empty() && !repository.contains('/') =>
        {
            Maybe::Present(RustdocLine::from(run))
        },
        Some(_) | None => Maybe::Absent(marker_check::Unmarked::NoLabel),
    }
}

/// The bytes a run of documentation lines carries.
///
/// # Specification
/// - ensures: returns the bytes of every line's text plus one per line break,
///   which is the documentation's text as one string with a final newline.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests measure an empty line, a multi-byte character
///   and the budget's cap.
/// - witness: `doc_shape::tests::the_budgets_admit_their_caps_and_refuse_one_byte_more`
#[spec(ensures: |output| output.0 == lines.iter().map(|line| line.text.len()).sum::<usize>().saturating_add(lines.len()))]
fn doc_bytes(lines: &[DocLine]) -> DocBytes
{
    DocBytes(lines.iter().fold(0_usize, |bytes, line| {
        bytes.saturating_add(line.text.len()).saturating_add(1)
    }))
}

/// A module's documentation measured by its structure.
///
/// # Specification
/// - ensures: the lead is the [`doc_bytes`] of the lines before the first
///   heading outside fenced code; each section runs from such a heading to the
///   line before the next, and carries its heading line, the heading as written
///   and its [`doc_bytes`]. The lead and the sections together are the
///   documentation's bytes.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests measure a lead, two sections, a fenced `#` line
///   inside a section and documentation without a heading.
/// - witness: `doc_shape::tests::the_budgets_admit_their_caps_and_refuse_one_byte_more`
#[spec(ensures: |output| output.sections.iter().fold(output.lead.0, |total, section| total.saturating_add(section.bytes.0)) == doc_bytes(lines).0)]
fn structure(lines: &[DocLine]) -> Structure
{
    let headings: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|&(_, line)| !line.code && heading_level(RustdocLine::from(&line.text)).0 > 0)
        .map(|(index, _)| index)
        .collect();
    let lead_end = headings.first().copied().unwrap_or(lines.len());
    let lead = doc_bytes(lines.get(.. lead_end).unwrap_or_default());
    let sections = headings
        .iter()
        .enumerate()
        .map(|(position, &start)| {
            let end = headings
                .get(position.saturating_add(1))
                .copied()
                .unwrap_or(lines.len());
            SectionSize {
                line: start,
                heading: lines
                    .get(start)
                    .map_or_else(String::new, |line| line.text.trim().to_owned()),
                bytes: doc_bytes(lines.get(start .. end).unwrap_or_default()),
            }
        })
        .collect();
    Structure { lead, sections }
}

/// Judge documentation bytes against a cap.
///
/// # Specification
/// - ensures: refuses with the bytes exactly when they exceed the cap.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate each cap and one byte over each.
/// - witness: `doc_shape::tests::the_budgets_admit_their_caps_and_refuse_one_byte_more`
#[spec(ensures: |output| matches!(output, Maybe::Present(_)) == (bytes.0 > cap.0))]
fn budget_judgement(
    bytes: DocBytes,
    cap: DocBytes,
) -> Maybe<DocBytes, budget_check::Within>
{
    if bytes.0 > cap.0 {
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
    use super::DocLine;
    use super::HeadingDefect;
    use super::HeadingState;
    use super::LEAD_BUDGET;
    use super::MODULE_BUDGET;
    use super::Part;
    use super::RecordKind;
    use super::SECTION_BUDGET;
    use super::budget_judgement;
    use super::classify;
    use super::decision_records;
    use super::doc_bytes;
    use super::heading_check::Admitted;
    use super::heading_judgement;
    use super::heading_level;
    use super::history_findings;
    use super::issue_references;
    use super::paragraphs;
    use super::structure;
    use crate::semantic::DocBytes;
    use crate::semantic::LineIndex;
    use crate::semantic::OuterDocumentation;
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

    /// The decision records a fixture's reading finds, as their line and form.
    ///
    /// # Specification
    /// trivial.
    fn records(
        text: RustdocText<'_>,
        outer: OuterDocumentation,
    ) -> Vec<(LineIndex, RecordKind)>
    {
        decision_records(&doc(text), outer)
            .into_iter()
            .map(|record| (LineIndex(record.line), record.kind))
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
    fn headings_are_judged_against_the_vocabulary()
    {
        let lines = doc(concat!(
            " # Examples  \n",
            " # Example\n",
            " # Exit codes\n",
            " # Boundaries\n",
            " # Specification\n",
            " # Specification\n",
            " ## Detail\n",
            " # Overview\n",
            " # Decision\n",
            " # The rule\n",
            " Prose.\n"
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
                Maybe::Absent(Admitted::Section),
                Maybe::Present(HeadingDefect::Repeated("Examples")),
                Maybe::Absent(Admitted::FoldIn),
                Maybe::Present(HeadingDefect::Repeated("Boundaries")),
                Maybe::Absent(Admitted::Evidence),
                Maybe::Absent(Admitted::Evidence),
                Maybe::Present(HeadingDefect::SubSection("## Detail".to_owned())),
                Maybe::Present(HeadingDefect::Refused(
                    "# Overview".to_owned(),
                    "the lead paragraph is the overview"
                )),
                Maybe::Present(HeadingDefect::Refused(
                    "# Decision".to_owned(),
                    super::DECISION_HOME
                )),
                Maybe::Present(HeadingDefect::OutsideVocabulary("# The rule".to_owned())),
                Maybe::Absent(Admitted::NotAHeading),
                Maybe::Absent(Admitted::NotAHeading),
            ],
            "a fold-in opens its section, and a section opens once under either spelling"
        );
    }

    #[test]
    fn labelled_runs_are_decision_records()
    {
        let labelled = |parts: &[Part]| RecordKind::Labelled(parts.to_vec());
        assert_eq!(
            records(
                concat!(
                    " - Choice: one.\n",
                    " - Alternatives: two.\n",
                    " - Reversal: three.\n",
                    " **The cache is per process.** A shared one costs more. Alternative: a\n",
                    " shared cache (Reversal: a slow rebuild).\n",
                    " Reversal: without its choice.\n",
                    " **Choice.** first; **Choice**: second.\n",
                    " The Reversal: inside a sentence. Reversals are no label.\n",
                    " `Reversal:` in a code span.\n",
                    " ```\n",
                    " Choice: fenced.\n",
                    " ```\n",
                    " |Choice|Alternative|Reversal condition|\n",
                    " |---|---|---|\n",
                    " |Choice of cache|one|none|\n",
                    " # Decisions\n"
                )
                .into(),
                OuterDocumentation(true)
            ),
            [
                (
                    LineIndex(0),
                    labelled(&[Part::Choice, Part::Alternatives, Part::Reversal])
                ),
                (
                    LineIndex(3),
                    labelled(&[Part::Alternatives, Part::Reversal])
                ),
                (LineIndex(5), labelled(&[Part::Reversal])),
                (LineIndex(6), labelled(&[Part::Choice])),
                (LineIndex(6), labelled(&[Part::Choice])),
                (LineIndex(12), RecordKind::Table),
                (LineIndex(15), RecordKind::Heading("# Decisions".to_owned())),
            ],
            "labels open sentences, a reversal closes its run, and a fence or a code span hides one"
        );
        assert_eq!(
            records(" # Decisions\n".into(), OuterDocumentation(false)),
            [],
            "a module's decision heading is the heading rule's"
        );
    }

    #[test]
    fn issue_numbers_in_prose_are_references()
    {
        let found: Vec<(usize, String)> = issue_references(&doc(concat!(
            " Fixed by #577, see (#12).\n",
            " Upstream gandr-lang/gandr#3 and #4.\n",
            " `#5` in a span, &#160; an entity, x#6 glued, #7a a word, #b a letter.\n",
            " ```\n",
            " #8\n",
            " ```\n",
            " a/b/c#9 is a path."
        )
        .into()))
        .into_iter()
        .map(|reference| (reference.line, reference.text))
        .collect();
        assert_eq!(
            found,
            [
                (0_usize, "#577".to_owned()),
                (0, "#12".to_owned()),
                (1, "gandr-lang/gandr#3".to_owned()),
                (1, "#4".to_owned()),
            ],
            "a number after a word, an entity, a span, a fence or a path is no reference"
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
    fn the_budgets_admit_their_caps_and_refuse_one_byte_more()
    {
        assert_eq!(
            doc_bytes(&doc(" é\n".into())).0,
            5_usize,
            "three bytes of text, an empty line, one break each"
        );
        for cap in [
            LEAD_BUDGET,
            SECTION_BUDGET,
            MODULE_BUDGET,
            CRATE_ROOT_BUDGET,
        ] {
            assert!(
                matches!(
                    budget_judgement(DocBytes(cap), DocBytes(cap)),
                    Maybe::Absent(_)
                ),
                "the cap itself is admitted"
            );
            assert_eq!(
                budget_judgement(DocBytes(cap.saturating_add(1)), DocBytes(cap)),
                Maybe::Present(DocBytes(cap.saturating_add(1))),
                "and one byte more is refused"
            );
        }
        let measured = structure(&doc(
            " Lead.\n\n # Items\n\n ```\n # fenced\n ```\n # References\n Cited.".into(),
        ));
        assert_eq!(
            (
                measured.lead.0,
                measured
                    .sections
                    .iter()
                    .map(|section| (section.line, section.heading.as_str(), section.bytes.0))
                    .collect::<Vec<_>>()
            ),
            (8_usize, vec![
                (2_usize, "# Items", 30_usize),
                (7, "# References", 22),
            ]),
            "the lead ends at the first heading, and a fenced `#` line stays in its section"
        );
        let unheaded = structure(&doc(" Lead only.".into()));
        assert_eq!(
            (unheaded.lead.0, unheaded.sections.len()),
            (12_usize, 0_usize),
            "documentation without a heading is all lead"
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
