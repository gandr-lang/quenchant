//! A source a program parses comes from a file, never from a Rust literal.
//!
//! A test that spells a multi-line source of another language as a Rust
//! string literal hides it from that language's formatter and from every
//! codemod that respells its sources. The consumer names the type its parser
//! reads, such as a source text wrapper, and this rule follows each value of
//! that type back to the literals it is built from. A literal of two or more
//! lines is refused. A source read with `include_str!` from a file of the
//! configured extension passes, and so does a one-line literal.
//!
//! The reading is over MIR, forward from each string constant of a body:
//! through moves, borrows, fields, arrays, tuples, `format!` and the calls
//! whose result keeps what they were handed. A conversion into the
//! configured type is a sink: `From::from` with the type as `Self`,
//! `Into::into` with the type as its target, and the type's own constructor.
//! A function whose parameter reaches a sink carries a summary, so a literal
//! handed to a helper is followed into it, in this crate or in a dependency
//! whose metadata holds its MIR. A first pass over the call graph reads each
//! callee's calls alone, and a callee that reaches no sink takes the empty
//! summary without the dataflow.
//!
//! Each library unit writes a reach record of its bodies beside its
//! metadata, and a later unit reads the reach of a dependency from it. The
//! rule reads the literals of a body only when the body reaches a sink.
//!
//! # Boundaries
//!
//! - Positions inside tuples, arrays and structures are kept to a depth of
//!   four, so a case table's expected text is not read as its source. A call
//!   the rule does not know mixes what it is handed.
//! - A value stored through a parameter's reference, a field of the receiver
//!   among them, is not followed out of the function that stores it. A source
//!   that reaches its sink only through state kept across calls is not seen.
//! - A closure handed to a call beside a value is taken to receive the value.
//! - Each inline source a sink reaches, one line or more, is written to the
//!   configured list directory, so a codemod reaches it.
//! - A dependency that no unit of this configuration lints, such as a crate
//!   from a registry, has no record, and the rule reads its bodies.
//!
//! # Items
//!
//! - [`EMBEDDED_SOURCE`]: the rule.
//! - [`EmbeddedSource`]: the pass and its configuration.
//! - [`Summaries`]: the parameters of each followed body that reach a sink.
//! - [`Reaching`]: whether each followed body can reach a sink at all.
//! - [`Record`]: what a library unit's reach record states of its crate.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Write as _;
use std::path::PathBuf;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use quenchant_shape::shape::Maybe;
use rustc_hir::HirId;
use rustc_hir::def::CtorKind;
use rustc_hir::def::CtorOf;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::CrateNum;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::DefIndex;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::mir::AggregateKind;
use rustc_middle::mir::Body;
use rustc_middle::mir::Const;
use rustc_middle::mir::ConstOperand;
use rustc_middle::mir::Local;
use rustc_middle::mir::Operand;
use rustc_middle::mir::Place;
use rustc_middle::mir::ProjectionElem;
use rustc_middle::mir::Rvalue;
use rustc_middle::mir::SourceInfo;
use rustc_middle::mir::StatementKind;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty;
use rustc_middle::ty::GenericArgsRef;
use rustc_middle::ty::Instance;
use rustc_middle::ty::InstanceKind;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TypingEnv;
use rustc_session::config::CrateType;
use rustc_session::config::OutFileName;
use rustc_session::config::OutputType;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::ExpnKind;
use rustc_span::MacroKind;
use rustc_span::Span;
use rustc_span::sym;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::answer_totality::names_local_crate;
use crate::answer_totality::report_defects;
use crate::answer_totality::resolve;
use crate::semantic::TypePath;
use crate::walk_summary::LIBRARY_CRATES;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a string literal of two or more lines that reaches a value of a configured type, such as the source text a parser reads: through a conversion into the type, through a helper whose parameter reaches one, in this crate or in a dependency, and through the tuples, arrays, `format!` and calls between. A source read with `include_str!` from a file of the configured extension is accepted, and so is a literal of one line, its last line break aside.
    ///
    /// ### Why is this bad?
    ///
    /// A source spelled as a Rust literal is outside the source language's formatter and outside every codemod that respells its sources: when the language moves, the literal keeps the old spelling until a test fails on it. A fixture file is a source those tools read, and `include_str!` gives the test the same bytes.
    ///
    /// ### Accepted forms
    ///
    /// `include_str!` of a file with the configured extension; a literal with no line break before its last byte; a string built at run time from no literal of two or more lines; and every package the configuration allows while its sources move.
    ///
    /// ### Clippy
    ///
    /// `needless_raw_string_hashes` and `needless_raw_strings` read a literal's spelling; no Clippy lint reads where a literal's value goes.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// let parsed = parse(SourceText::from("def a =\n  1\n"));
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// let parsed = parse(SourceText::from(include_str!("fixtures/a.gandr")));
    /// ```
    pub EMBEDDED_SOURCE,
    Deny,
    "a multi-line source a configured type reads is spelled as a Rust literal"
}

/// The table, under `[quenchant-dylints]`, that configures the rule.
const TABLE: &str = "embedded_source";

/// The deepest position kept inside a value.
const DEPTH: usize = 4;

/// The extension of a library unit's reach record, beside its metadata.
const RECORD_EXTENSION: &str = "embedded-reach";

/// The first line of a reach record, which names its format.
const RECORD_FORMAT: &str = "quenchant embedded_source reach 1";

quenchant_shape::reason_enum! {
    /// The configuration can leave the rule off.
    mod setting {
        /// Why the rule reads nothing in this crate.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unconfigured {
            /// No `embedded_source` table is configured.
            NoTable,
        }
    }
}

quenchant_shape::reason_enum! {
    /// The configuration can name no directory for the list of inline sources.
    mod listing {
        /// Why no list is written.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unlisted {
            /// The table names no `directory`.
            NoDirectory,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A constant can be other than a string a source is spelled in.
    mod constant_reading {
        /// Why the constant seeds nothing.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotText {
            /// The constant is not a string or the template of a `format!`.
            OtherType,
            /// The constant's bytes cannot be read.
            Unreadable,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A call can resolve to no body a summary reads.
    mod callee_reading {
        /// Why the callee is not followed.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unfollowed {
            /// The call goes through a pointer or names no function item.
            NotAnItem,
            /// The callee is in the standard library, which builds no
            /// configured type.
            Library,
            /// The callee resolves to no single body, or its MIR is not in the
            /// metadata.
            NoBody,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A dependency's body can have no reach its crate's record states.
    mod recording {
        /// Why a body's reach is not read from a record.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrecorded {
            /// The body is in the crate being linted.
            Local,
            /// The definition is no function or closure, or its MIR is not in
            /// the metadata.
            NoBody,
            /// The crate's metadata is in no file, so no record stands beside
            /// it.
            NoArtifact,
            /// The crate's unit wrote no record, or its record is of another
            /// build of the crate or of another configuration.
            NoRecord,
        }
    }
}

/// One step into a value: a field, or any element of an array or slice.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Step
{
    /// The field of this index, of a tuple, a structure, a variant or a
    /// closure's captures.
    Field(u32),
    /// An element of an array, a slice or an iterator over one.
    Element,
}

/// A position inside a value, outermost step first, at most [`DEPTH`] steps.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Path
{
    /// The steps, the first `length` of them used.
    steps: [Step; DEPTH],
    /// How many steps are used.
    length: Depth,
}

/// A count of steps, at most [`DEPTH`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
struct Depth(usize);

/// Whether one position lies inside, around or beside another.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Overlap
{
    /// One position is a prefix of the other.
    Overlaps,
    /// The positions part at some step.
    Apart,
}

impl Path
{
    /// The whole value.
    const WHOLE: Self = Self {
        steps: [Step::Element; DEPTH],
        length: Depth(0),
    };

    /// The used steps.
    ///
    /// # Specification
    /// trivial.
    fn steps(&self) -> &[Step]
    {
        self.steps.get(.. self.length.0).unwrap_or(&[])
    }

    /// This position inside `step` of an enclosing value.
    ///
    /// # Specification
    /// - ensures: the steps are `step` then this position's, the last dropped
    ///   when they would exceed [`DEPTH`]; a dropped step widens the position
    ///   to its prefix, which overlaps everything the step did.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures build arrays of tuples and read one
    ///   field of each element; only the field read is refused.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| output.steps().first() == Some(&step) && output.length.0 <= DEPTH)]
    fn inside(
        self,
        step: Step,
    ) -> Self
    {
        let mut steps = [Step::Element; DEPTH];
        let mut length = 0_usize;
        for (slot, held) in steps
            .iter_mut()
            .zip(core::iter::once(step).chain(self.steps().iter().copied()))
        {
            *slot = held;
            length = length.saturating_add(1);
        }
        Self {
            steps,
            length: Depth(length),
        }
    }

    /// This position with `step` appended, the step dropped past [`DEPTH`].
    ///
    /// # Specification
    /// - ensures: the steps are this position's then `step`, unless the
    ///   position is already [`DEPTH`] long, when it is unchanged.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures hand a helper a tuple whose second field
    ///   the helper converts; the first field's literal is not refused.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| output.length.0 == self.length.0.saturating_add(1).min(DEPTH))]
    fn then(
        self,
        step: Step,
    ) -> Self
    {
        let mut extended = self;
        if let Some(slot) = extended.steps.get_mut(self.length.0) {
            *slot = step;
            extended.length = Depth(self.length.0.saturating_add(1));
        }
        extended
    }

    /// The position under its first step, when that step is `step`.
    ///
    /// # Specification
    /// - ensures: `Present` with the remaining steps when the first step is
    ///   `step`; `Absent` when the first step differs; the whole value stays
    ///   the whole value.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures read the first field of a tuple whose
    ///   second holds a multi-line expected text, which is not refused.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| match output {
        Maybe::Present(rest) => rest.length.0 == self.length.0.saturating_sub(1),
        Maybe::Absent(_) => self.steps().first().is_some_and(|first| *first != step),
    })]
    fn under(
        self,
        step: Step,
    ) -> Maybe<Self, Overlap>
    {
        let Some((&first, rest)) = self.steps().split_first()
        else {
            return Maybe::Present(self);
        };
        if first != step {
            return Maybe::Absent(Overlap::Apart);
        }
        let mut steps = [Step::Element; DEPTH];
        for (slot, &held) in steps.iter_mut().zip(rest) {
            *slot = held;
        }
        Maybe::Present(Self {
            steps,
            length: Depth(rest.len()),
        })
    }

    /// Whether the two positions overlap: one is a prefix of the other.
    ///
    /// # Specification
    /// - ensures: [`Overlap::Overlaps`] exactly when the shorter position's
    ///   steps begin the longer one's.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures hand helpers tuples whose fields are
    ///   read and not read, and whole values.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| (output == Overlap::Overlaps) == self.steps().iter().zip(other.steps()).all(|(left, right)| left == right))]
    fn overlap(
        self,
        other: Self,
    ) -> Overlap
    {
        if self
            .steps()
            .iter()
            .zip(other.steps())
            .all(|(left, right)| left == right)
        {
            Overlap::Overlaps
        }
        else {
            Overlap::Apart
        }
    }

    /// The steps of `self` past the length of `prefix`, appended to `base`.
    ///
    /// # Specification
    /// - ensures: `base` with each step of `self` after the first
    ///   `prefix.length` appended in order, as [`Path::then`] appends.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures hand a helper a whole tuple whose helper
    ///   hands one field on to a second helper that converts it.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| output.length.0 >= base.length.0)]
    fn beyond(
        self,
        prefix: Self,
        base: Self,
    ) -> Self
    {
        self.steps()
            .iter()
            .skip(prefix.length.0)
            .fold(base, |path, &step| path.then(step))
    }
}

/// A literal a body holds, which may reach a sink.
#[derive(Clone, Debug)]
struct Seed
{
    /// Where the literal is written, or the call site of the macro that
    /// wrote it.
    span: Span,
    /// The node whose lint level the finding takes.
    lint_root: HirId,
    /// The literal's text.
    text: String,
    /// Where the text comes from.
    origin: Origin,
}

/// Where a seed's text comes from.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Origin
{
    /// A literal, a `concat!` or a `format!` template in the source.
    Literal,
    /// `include_str!` of the named path.
    Included(String),
}

/// The index of a seed in the crate's table.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
struct SeedId(usize);

/// The index of a parameter of a body, the closure environment first in a
/// closure's.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
struct Parameter(usize);

/// What a value holds: a literal of the body, or part of a parameter.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Held
{
    /// A literal of this body or of a constant it reads.
    Literal(SeedId),
    /// Part of a parameter, at `from` inside it.
    Parameter
    {
        /// The parameter.
        parameter: Parameter,
        /// The position inside the parameter the value was read from.
        from: Path,
    },
}

/// One thing a local holds, and where inside the local's value it sits.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Fact
{
    /// What is held.
    held: Held,
    /// Where it sits inside the value.
    at: Path,
}

impl Fact
{
    /// The fact once `step` reads into the value.
    ///
    /// # Specification
    /// - ensures: `Absent` when the held thing sits apart from `step`; under
    ///   `step` when it sits inside it; when it is the whole value, a
    ///   parameter's part moves to `from` extended by `step` and a literal
    ///   stays as it is.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures read fields of tuples of literals and
    ///   fields of a parameter.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| match output {
        Maybe::Present(read) => read.held == self.held || matches!(self.held, Held::Parameter { .. }),
        Maybe::Absent(_) => self.at.length.0 > 0,
    })]
    fn read(
        self,
        step: Step,
    ) -> Maybe<Self, Overlap>
    {
        if self.at.length.0 > 0 {
            return match self.at.under(step) {
                Maybe::Present(at) => Maybe::Present(Self { at, ..self }),
                Maybe::Absent(apart) => Maybe::Absent(apart),
            };
        }
        match self.held {
            Held::Literal(_) => Maybe::Present(self),
            Held::Parameter { parameter, from } => Maybe::Present(Self {
                held: Held::Parameter {
                    parameter,
                    from: from.then(step),
                },
                at: Path::WHOLE,
            }),
        }
    }

    /// The fact with its position forgotten: the held thing is somewhere in
    /// the value.
    ///
    /// # Specification
    /// trivial.
    const fn mixed(self) -> Self
    {
        Self {
            held: self.held,
            at: Path::WHOLE,
        }
    }
}

/// What each local of a body holds, and what each operand of a call or of a
/// configured aggregate is handed.
struct Facts
{
    /// What each local holds, by its index.
    locals: Vec<BTreeSet<Fact>>,
    /// What each operand holds at the call or aggregate at a location, read
    /// in the last pass of the settling, so constants are seeded too.
    handed: HashMap<rustc_middle::mir::Location, Vec<BTreeSet<Fact>>>,
}

/// Whether a set of facts grew.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Grew
{
    /// A fact was added.
    Grew,
    /// Nothing changed.
    Same,
}

impl Facts
{
    /// The facts of `local`.
    ///
    /// # Specification
    /// trivial.
    fn of(
        &self,
        local: Local,
    ) -> BTreeSet<Fact>
    {
        self.locals
            .get(local.as_usize())
            .cloned()
            .unwrap_or_default()
    }

    /// What each operand holds at the call or aggregate at `location`.
    ///
    /// # Specification
    /// trivial.
    fn handed_at(
        &self,
        location: rustc_middle::mir::Location,
    ) -> &[BTreeSet<Fact>]
    {
        self.handed.get(&location).map_or(&[], Vec::as_slice)
    }

    /// Add `facts` to `local`.
    ///
    /// # Specification
    /// - ensures: `local` holds a superset of `facts` afterwards; the output is
    ///   [`Grew::Grew`] exactly when it held fewer before.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures carry literals through loops, which
    ///   settle only by repetition.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |grew| grew == Grew::Same || self.locals.get(local.as_usize()).is_some_and(|held| !held.is_empty()))]
    fn extend(
        &mut self,
        local: Local,
        facts: BTreeSet<Fact>,
    ) -> Grew
    {
        let Some(held) = self.locals.get_mut(local.as_usize())
        else {
            return Grew::Same;
        };
        let before = held.len();
        held.extend(facts);
        if held.len() == before {
            Grew::Same
        }
        else {
            Grew::Grew
        }
    }
}

/// Whether a body is read for its literals or for its parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Reading
{
    /// The body's string constants are seeds; a reached sink refuses them.
    Literals,
    /// The body's parameters are seeds; a reached sink is its summary.
    Parameters,
}

/// The configured rule.
struct Configured
{
    /// The types a sink converts into.
    types: HashSet<DefId>,
    /// The configured type paths, sorted, as a reach record names them: a
    /// unit reads only a record of the same paths.
    heading: Heading,
    /// The extension of a file `include_str!` may read a source from.
    extension: String,
    /// Whether this package's findings are allowed while its sources move.
    allowance: Allowance,
    /// Where the list of inline sources is written.
    directory: Maybe<PathBuf, listing::Unlisted>,
    /// The configuration's defects, reported in the crate it names.
    defects: Vec<String>,
}

/// The configured type paths, sorted and once each, after the word `types`:
/// the line a reach record carries to name its configuration.
#[derive(Clone)]
#[repr(transparent)]
struct Heading(String);

/// Whether a package's findings are reported.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Allowance
{
    /// The configuration allows the package; nothing is reported.
    Allowed,
    /// Every finding is reported.
    Held,
}

/// The rule's pass, with the configuration it read.
#[repr(transparent)]
pub struct EmbeddedSource
{
    /// The configuration, or why there is none.
    configured: Maybe<Configured, setting::Unconfigured>,
}

impl_lint_pass!(EmbeddedSource => [EMBEDDED_SOURCE]);

impl EmbeddedSource
{
    /// Read the rule's table from the Dylint configuration.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: `Present` exactly when the `embedded_source` table exists;
    ///   its `types` resolve to their definitions, a type naming this crate and
    ///   resolving to nothing becoming a defect; the heading names each path
    ///   `types` spells, sorted and once; `extension` defaults to `gandr`; the
    ///   package is allowed when `allowed` names `CARGO_PKG_NAME`.
    /// - panics: none.
    /// - executable: none — the configuration is read from outside the
    ///   compilation; reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures configure a type of their own crate and
    ///   one of a dependency, an allowed package and a list directory.
    /// - witness: `tests::ui_embedded_source`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
        else {
            return Self {
                configured: Maybe::Absent(setting::Unconfigured::NoTable),
            };
        };
        let Some(table) = config.get(TABLE)
        else {
            return Self {
                configured: Maybe::Absent(setting::Unconfigured::NoTable),
            };
        };
        let mut defects = Vec::new();
        let mut types = HashSet::new();
        let mut paths = BTreeSet::new();
        for entry in table
            .get("types")
            .and_then(|types| types.as_array())
            .into_iter()
            .flatten()
        {
            let Some(path) = entry.as_str()
            else {
                defects.push(format!(
                    "`{TABLE}.types` holds `{entry}`, which is not a path"
                ));
                continue;
            };
            paths.insert(path);
            let found: Vec<DefId> = resolve(tcx, TypePath(path))
                .into_iter()
                .filter(|&def_id| matches!(tcx.def_kind(def_id), DefKind::Struct | DefKind::Enum))
                .collect();
            if found.is_empty() && names_local_crate(tcx, TypePath(path)).0 {
                defects.push(format!(
                    "`{TABLE}.types` names `{path}`, which resolves to no structure or enum"
                ));
            }
            types.extend(found);
        }
        let extension = table
            .get("extension")
            .and_then(|extension| extension.as_str())
            .unwrap_or("gandr")
            .to_owned();
        let package = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
        let allowed = table
            .get("allowed")
            .and_then(|allowed| allowed.as_array())
            .into_iter()
            .flatten()
            .any(|entry| entry.as_str() == Some(package.as_str()));
        let directory = table
            .get("directory")
            .and_then(|directory| directory.as_str())
            .map_or(Maybe::Absent(listing::Unlisted::NoDirectory), |directory| {
                Maybe::Present(PathBuf::from(directory))
            });
        let heading = Heading(paths.into_iter().fold(
            String::from("types"),
            |mut heading, path| {
                heading.push(' ');
                heading.push_str(path);
                heading
            },
        ));
        Self {
            configured: Maybe::Present(Configured {
                types,
                heading,
                extension,
                allowance: if allowed {
                    Allowance::Allowed
                }
                else {
                    Allowance::Held
                },
                directory,
                defects,
            }),
        }
    }
}

/// Where a reached sink stands.
#[derive(Clone, Copy, Debug)]
struct Witness
{
    /// The conversion into the configured type.
    span: Span,
    /// The function whose body converts.
    owner: DefId,
}

/// The positions of each parameter of a body that reach a sink.
#[derive(Clone, Debug, Default)]
#[repr(transparent)]
struct Summary(BTreeMap<Parameter, (BTreeSet<Path>, Witness)>);

impl Summary
{
    /// Record that `from` inside `parameter` reaches the sink at `witness`.
    ///
    /// # Specification
    /// - ensures: `parameter` holds `from` afterwards, with its first witness.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures' helpers convert one parameter and pass
    ///   another through.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |_| self.0.get(&parameter).is_some_and(|(paths, _)| paths.contains(&from)))]
    fn record(
        &mut self,
        parameter: Parameter,
        from: Path,
        witness: Witness,
    )
    {
        self.0
            .entry(parameter)
            .or_insert_with(|| (BTreeSet::new(), witness))
            .0
            .insert(from);
    }
}

/// The summaries computed so far in one compilation, by body.
#[derive(Default)]
#[repr(transparent)]
pub struct Summaries(HashMap<DefId, Summary>);

/// The outcome of reading one body against the summaries so far.
enum Scan
{
    /// Every summary the reading needs is computed.
    Ready(Vec<(Fact, Witness)>),
    /// These bodies must be summarized first.
    Needs(Vec<DefId>),
}

/// What a library unit's reach record states of its crate's bodies.
///
/// Each library unit writes its record beside its metadata. A later unit
/// decides the body of a dependency from that record and does not read the
/// body. So the unit of each crate reads the call graph of the crate once in
/// a workspace.
#[derive(Debug)]
enum Record
{
    /// No configured type resolves in the crate's graph, so no body of the
    /// crate can reach a sink.
    Blind,
    /// The indices of the crate's bodies that reach a sink; every other
    /// function and closure body of the crate reaches none.
    Sinking(HashSet<DefIndex>),
}

/// Whether each followed body can reach a sink, directly or through the
/// bodies it calls.
///
/// A body that reaches no sink has an empty summary, so no dataflow over it
/// is needed. The reach reads each body once, its calls and its aggregates
/// alone, where a summary settles every local of the body. When a
/// dependency's library unit wrote a record of this configuration, the record
/// decides the dependency's bodies, and the reach reads none of them.
struct Reaching
{
    /// The answer for each body decided so far.
    decided: HashMap<DefId, Sink>,
    /// The record of each dependency met so far, or why it has none.
    records: HashMap<CrateNum, Maybe<Record, recording::Unrecorded>>,
    /// The heading a record carries when it is of this configuration.
    heading: Heading,
}

impl Reaching
{
    /// No body decided yet, and records read only under `heading`.
    ///
    /// # Specification
    /// trivial.
    fn new(heading: Heading) -> Self
    {
        Self {
            decided: HashMap::new(),
            records: HashMap::new(),
            heading,
        }
    }

    /// Whether the body of `root` can reach a sink.
    ///
    /// # Specification
    /// - ensures: [`Sink::Sink`] exactly when a body the call graph reaches
    ///   from `root`, `root` included, constructs a configured type or calls a
    ///   conversion into one; every body explored is decided afterwards.
    /// - panics: none.
    /// - economy: each body is read once in a compilation, for its calls and
    ///   aggregates, and a dependency's body that [`Self::recorded`] answers is
    ///   not read; the graph of the bodies not yet decided is held only while
    ///   they are decided.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures convert through a helper that hands its
    ///   argument to another, in this crate and in a dependency: deciding the
    ///   outer helper before the inner one drops the helper's summary.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |ret| self.decided.get(&root) == Some(&ret))]
    fn reaches(
        &mut self,
        tcx: TyCtxt<'_>,
        types: &HashSet<DefId>,
        root: DefId,
    ) -> Sink
    {
        if let Some(&known) = self.decided.get(&root) {
            return known;
        }
        // The bodies reachable from `root` that are not decided yet, each
        // with the bodies it calls, in the order they are explored, and the
        // ones that reach a sink directly.
        let mut explored: HashSet<DefId> = HashSet::new();
        let mut edges: Vec<(DefId, Vec<DefId>)> = Vec::new();
        let mut reaching: Vec<DefId> = Vec::new();
        let mut stack = vec![root];
        while let Some(current) = stack.pop() {
            if self.decided.contains_key(&current) || !explored.insert(current) {
                continue;
            }
            if let Maybe::Present(recorded) = self.recorded(tcx, current) {
                self.decided.insert(current, recorded);
                continue;
            }
            let (callees, sinks) = calls_of(tcx, types, current);
            if sinks == Sink::Sink {
                reaching.push(current);
            }
            stack.extend(
                callees.iter().copied().filter(|callee| {
                    !self.decided.contains_key(callee) && !explored.contains(callee)
                }),
            );
            edges.push((current, callees));
        }
        let mut callers: HashMap<DefId, Vec<DefId>> = HashMap::new();
        for &(caller, ref callees) in &edges {
            for &callee in callees {
                if self.decided.get(&callee) == Some(&Sink::Sink) {
                    reaching.push(caller);
                }
                callers.entry(callee).or_default().push(caller);
            }
        }
        let mut reached: HashSet<DefId> = HashSet::new();
        while let Some(body) = reaching.pop() {
            if reached.insert(body)
                && let Some(found) = callers.get(&body)
            {
                reaching.extend(found.iter().copied());
            }
        }
        for &(body, _) in &edges {
            let reach = if reached.contains(&body) {
                Sink::Sink
            }
            else {
                Sink::Other
            };
            self.decided.insert(body, reach);
        }
        self.decided.get(&root).copied().unwrap_or(Sink::Other)
    }

    /// The reach of a dependency's body, as its crate's record states it.
    ///
    /// # Specification
    /// - ensures: `Present` for a function or closure of a dependency whose MIR
    ///   is in the metadata, when [`read_record`] finds the crate's record:
    ///   [`Sink::Other`] for a blind crate, and [`Sink::Sink`] exactly when the
    ///   record lists the body otherwise; `Absent` for any other body. The
    ///   reach reads each crate's record at most once.
    /// - panics: none.
    /// - executable: none — the outcome is a file another compilation wrote; a
    ///   predicate could only read it again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures convert through a dependency's helper,
    ///   which its own unit records as reaching a sink: a record that leaves
    ///   the helper out loses the refusal.
    /// - witness: `tests::ui_embedded_source`
    fn recorded(
        &mut self,
        tcx: TyCtxt<'_>,
        def_id: DefId,
    ) -> Maybe<Sink, recording::Unrecorded>
    {
        if def_id.is_local() {
            return Maybe::Absent(recording::Unrecorded::Local);
        }
        if !matches!(
            tcx.def_kind(def_id),
            DefKind::Fn | DefKind::AssocFn | DefKind::Closure
        ) || !tcx.is_mir_available(def_id)
        {
            return Maybe::Absent(recording::Unrecorded::NoBody);
        }
        let heading = &self.heading;
        let record = self
            .records
            .entry(def_id.krate)
            .or_insert_with(|| read_record(tcx, def_id.krate, heading));
        match *record {
            Maybe::Present(Record::Blind) => Maybe::Present(Sink::Other),
            Maybe::Present(Record::Sinking(ref indices)) => {
                Maybe::Present(if indices.contains(&def_id.index) {
                    Sink::Sink
                }
                else {
                    Sink::Other
                })
            },
            Maybe::Absent(reason) => Maybe::Absent(reason),
        }
    }
}

/// The reach record a dependency's library unit wrote beside its metadata.
///
/// # Specification
/// - ensures: `Present` when the file beside the crate's metadata, rlib or
///   dylib holds the record format, the crate's hash and `heading`, one on a
///   line, then `blind`, or `sinking` and one body index on each line after it;
///   `Absent` otherwise, as for a crate no unit linted.
/// - panics: none.
/// - executable: none — the outcome is a file another compilation wrote; a
///   predicate could only read it again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures' dependency is linted before them and writes
///   the record they read.
/// - witness: `tests::ui_embedded_source`
fn read_record(
    tcx: TyCtxt<'_>,
    krate: CrateNum,
    heading: &Heading,
) -> Maybe<Record, recording::Unrecorded>
{
    let source = tcx.used_crate_source(krate);
    let Some(artifact) = source
        .rmeta
        .as_ref()
        .or(source.rlib.as_ref())
        .or(source.dylib.as_ref())
    else {
        return Maybe::Absent(recording::Unrecorded::NoArtifact);
    };
    let Ok(text) = std::fs::read_to_string(artifact.with_extension(RECORD_EXTENSION))
    else {
        return Maybe::Absent(recording::Unrecorded::NoRecord);
    };
    let built = format!("crate {:032x}", tcx.crate_hash(krate).as_u128());
    let mut lines = text.lines();
    if lines.next() != Some(RECORD_FORMAT)
        || lines.next() != Some(built.as_str())
        || lines.next() != Some(heading.0.as_str())
    {
        return Maybe::Absent(recording::Unrecorded::NoRecord);
    }
    match lines.next() {
        Some("blind") => Maybe::Present(Record::Blind),
        Some("sinking") => {
            let mut indices = HashSet::new();
            for line in lines {
                let Ok(index) = line.parse::<u32>()
                else {
                    return Maybe::Absent(recording::Unrecorded::NoRecord);
                };
                indices.insert(DefIndex::from_u32(index));
            }
            Maybe::Present(Record::Sinking(indices))
        },
        _ => Maybe::Absent(recording::Unrecorded::NoRecord),
    }
}

/// The bodies a body's calls follow, and whether the body reaches a sink
/// itself.
///
/// # Specification
/// - ensures: the followed callee of each call and each closure handed to a
///   call, as [`scan`] looks them up; [`Sink::Sink`] when the body constructs a
///   configured type or calls a conversion into one. A body whose MIR is not
///   available calls nothing and reaches no sink.
/// - panics: none.
/// - executable: none — the outcome is a read of the body's MIR; a predicate
///   could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — as [`Reaching::reaches`].
/// - witness: `tests::ui_embedded_source`
fn calls_of(
    tcx: TyCtxt<'_>,
    types: &HashSet<DefId>,
    def_id: DefId,
) -> (Vec<DefId>, Sink)
{
    let mut callees = Vec::new();
    if !tcx.is_mir_available(def_id) {
        return (callees, Sink::Other);
    }
    let body = tcx.optimized_mir(def_id);
    let typing_env = TypingEnv::post_analysis(tcx, def_id);
    let mut sinks = Sink::Other;
    for block in body.basic_blocks.iter() {
        for statement in &block.statements {
            if let StatementKind::Assign(ref assignment) = statement.kind
                && let Rvalue::Aggregate(ref kind, _) = assignment.1
                && let AggregateKind::Adt(adt, ..) = **kind
                && types.contains(&adt)
            {
                sinks = Sink::Sink;
            }
        }
        let Some(terminator) = block.terminator.as_ref()
        else {
            continue;
        };
        let TerminatorKind::Call {
            ref func, ref args, ..
        } = terminator.kind
        else {
            continue;
        };
        let Some((callee, callee_args)) = func.const_fn_def()
        else {
            continue;
        };
        if sink_call(tcx, types, callee, callee_args) == Sink::Sink {
            sinks = Sink::Sink;
            continue;
        }
        for argument in args {
            if let ty::Closure(closure, _) =
                *argument.node.ty(&body.local_decls, tcx).peel_refs().kind()
            {
                callees.push(closure);
            }
        }
        if let Maybe::Present(target) = followed(tcx, typing_env, callee, callee_args) {
            callees.push(target);
        }
    }
    (callees, sinks)
}

impl Summaries
{
    /// Compute the summary of `def_id` and of every body it reaches.
    ///
    /// # Specification
    /// - ensures: the summary of `def_id` is recorded afterwards; a body met
    ///   again while its own reading is pending reaches nothing, so a call
    ///   cycle ends; a body that `reaching` finds reaches no sink takes the
    ///   empty summary without a reading.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures hand literals to a helper that hands
    ///   them to another, in this crate and in a dependency.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |_| self.0.contains_key(&def_id))]
    fn compute(
        &mut self,
        tcx: TyCtxt<'_>,
        types: &HashSet<DefId>,
        reaching: &mut Reaching,
        def_id: DefId,
    )
    {
        let mut pending = vec![def_id];
        let mut reading = HashSet::new();
        while let Some(&current) = pending.last() {
            if self.0.contains_key(&current) {
                pending.pop();
                reading.remove(&current);
                continue;
            }
            if reaching.reaches(tcx, types, current) == Sink::Other {
                self.0.insert(current, Summary::default());
                pending.pop();
                continue;
            }
            let body = tcx.optimized_mir(current);
            let mut seeds = Seeds::default();
            let facts = settle(tcx, types, body, Reading::Parameters, &mut seeds);
            match scan(tcx, types, body, current, &facts, &self.0, &reading) {
                Scan::Ready(hits) => {
                    let mut summary = Summary::default();
                    for (fact, witness) in hits {
                        if let Held::Parameter { parameter, from } = fact.held {
                            summary.record(parameter, from, witness);
                        }
                    }
                    self.0.insert(current, summary);
                    pending.pop();
                    reading.remove(&current);
                },
                Scan::Needs(callees) => {
                    reading.insert(current);
                    pending.extend(callees);
                },
            }
        }
    }
}

/// The seeds of the crate: every literal a read body holds.
#[derive(Default)]
struct Seeds
{
    /// The seeds, by index.
    table: Vec<Seed>,
    /// Each seed's index, by its span, so a literal met twice is one seed.
    by_span: HashMap<Span, SeedId>,
    /// What each constant item and promoted constant returns, once read.
    constants: HashMap<(DefId, Promotion), BTreeSet<Fact>>,
}

/// Which body of a definition a constant names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Promotion
{
    /// The definition's own body.
    Item,
    /// The promoted constant of this index inside the definition.
    Promoted(u32),
}

impl Seeds
{
    /// The seed of a literal at `span`, added when it is new.
    ///
    /// # Specification
    /// - ensures: one seed per span; a span met before gives its first seed.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures read one constant item from two bodies
    ///   and refuse its literal once.
    /// - witness: `tests::ui_embedded_source`
    #[spec(ensures: |output| self.table.get(output.0).is_some())]
    fn seed(
        &mut self,
        seed: Seed,
    ) -> SeedId
    {
        if let Some(&known) = self.by_span.get(&seed.span) {
            return known;
        }
        let id = SeedId(self.table.len());
        self.by_span.insert(seed.span, id);
        self.table.push(seed);
        id
    }
}

/// The facts a constant operand holds.
///
/// # Specification
/// - requires: [`read_constants`] has read every constant item and promoted
///   constant the body holds.
/// - ensures: a string constant, or a `format!` template, holds one literal
///   seed at the whole value; a constant item or a promoted constant holds what
///   [`read_constants`] recorded its body returns; any other constant holds
///   nothing.
/// - panics: none.
/// - executable: none — the outcome is the compiler's constant and source map,
///   which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert a literal, a constant item, a
///   promoted array, `concat!`, `format!` and `include_str!`.
/// - witness: `tests::ui_embedded_source`
fn constant_facts<'tcx>(
    tcx: TyCtxt<'tcx>,
    constant: &ConstOperand<'tcx>,
    site: Site,
    seeds: &mut Seeds,
) -> BTreeSet<Fact>
{
    match constant.const_ {
        Const::Unevaluated(unevaluated, _) => seeds
            .constants
            .get(&constant_key(unevaluated.def, unevaluated.promoted))
            .cloned()
            .unwrap_or_default(),
        Const::Val(..) | Const::Ty(..) => match literal(tcx, constant, site) {
            Maybe::Present(seed) => BTreeSet::from([Fact {
                held: Held::Literal(seeds.seed(seed)),
                at: Path::WHOLE,
            }]),
            Maybe::Absent(_) => BTreeSet::new(),
        },
    }
}

/// The key a constant item or a promoted constant is recorded under.
///
/// # Specification
/// trivial.
fn constant_key(
    def_id: DefId,
    promoted: Option<rustc_middle::mir::Promoted>,
) -> (DefId, Promotion)
{
    (
        def_id,
        promoted.map_or(Promotion::Item, |index| Promotion::Promoted(index.as_u32())),
    )
}

/// The constant items and promoted constants a body reads.
#[derive(Default)]
#[repr(transparent)]
struct ConstantsIn(Vec<(DefId, Promotion)>);

impl<'tcx> rustc_middle::mir::visit::Visitor<'tcx> for ConstantsIn
{
    /// Note each unevaluated constant the body names.
    ///
    /// # Specification
    /// - ensures: an unevaluated constant's key is appended; any other constant
    ///   is passed over.
    /// - panics: none.
    /// - executable: none — the outcome is the visited body's constants.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures read a constant item from two bodies and
    ///   a promoted array of literals.
    /// - witness: `tests::ui_embedded_source`
    fn visit_const_operand(
        &mut self,
        constant: &ConstOperand<'tcx>,
        _: rustc_middle::mir::Location,
    )
    {
        if let Const::Unevaluated(unevaluated, _) = constant.const_ {
            self.0
                .push(constant_key(unevaluated.def, unevaluated.promoted));
        }
    }
}

/// The body a constant key names, when its MIR is readable.
///
/// # Specification
/// - ensures: a promoted constant's body inside its definition; a constant
///   item's body for evaluation when its MIR is available; `Absent` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's MIR queries.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures read a constant item and a promoted array.
/// - witness: `tests::ui_embedded_source`
fn constant_body(
    tcx: TyCtxt<'_>,
    (def_id, promotion): (DefId, Promotion),
) -> Maybe<&Body<'_>, constant_reading::NotText>
{
    match promotion {
        Promotion::Promoted(index) => tcx
            .promoted_mir(def_id)
            .iter_enumerated()
            .find(|&(promoted, _)| promoted.as_u32() == index)
            .map_or(
                Maybe::Absent(constant_reading::NotText::Unreadable),
                |(_, body)| Maybe::Present(body),
            ),
        Promotion::Item
            if matches!(
                tcx.def_kind(def_id),
                DefKind::Const { .. } | DefKind::AssocConst { .. }
            ) && tcx.is_mir_available(def_id) =>
        {
            Maybe::Present(tcx.mir_for_ctfe(def_id))
        },
        Promotion::Item => Maybe::Absent(constant_reading::NotText::OtherType),
    }
}

/// Read what every constant a body names returns, constants it names first.
///
/// # Specification
/// - ensures: every constant item and promoted constant `body` names, and every
///   one those bodies name in turn, is recorded in `seeds` with what its body
///   returns; a constant whose body cannot be read, or one met again while its
///   reading is pending, returns nothing.
/// - panics: none.
/// - executable: none — the outcome is a fixpoint over constant bodies; a
///   predicate could only recompute it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert a constant item and a promoted array
///   of literals.
/// - witness: `tests::ui_embedded_source`
fn read_constants<'tcx>(
    tcx: TyCtxt<'tcx>,
    types: &HashSet<DefId>,
    body: &Body<'tcx>,
    seeds: &mut Seeds,
)
{
    let named = |body: &Body<'tcx>| {
        let mut found = ConstantsIn::default();
        rustc_middle::mir::visit::Visitor::visit_body(&mut found, body);
        found.0
    };
    let mut pending = named(body);
    let mut reading = HashSet::new();
    while let Some(&key) = pending.last() {
        if seeds.constants.contains_key(&key) {
            pending.pop();
            reading.remove(&key);
            continue;
        }
        let Maybe::Present(held) = constant_body(tcx, key)
        else {
            seeds.constants.insert(key, BTreeSet::new());
            pending.pop();
            continue;
        };
        let missing: Vec<(DefId, Promotion)> = named(held)
            .into_iter()
            .filter(|inner| !seeds.constants.contains_key(inner) && !reading.contains(inner))
            .collect();
        if missing.is_empty() || reading.contains(&key) {
            let returned =
                settle(tcx, types, held, Reading::Literals, seeds).of(Local::from_u32(0));
            seeds.constants.insert(key, returned);
            pending.pop();
            reading.remove(&key);
        }
        else {
            reading.insert(key);
            pending.extend(missing);
        }
    }
}

/// The literal a string constant spells.
///
/// # Specification
/// - ensures: `Present` for a `&str` constant with its bytes as text, its
///   origin [`Origin::Included`] with the path when an `include_str!` expansion
///   wrote it and [`Origin::Literal`] otherwise, its span moved to the
///   outermost macro call site that is not `include_str!`; `Present` for a
///   `&[u8]` constant handed to a call a macro wrote, as a `format!` template
///   is, with the text of the first string literal of the macro call; `Absent`
///   otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's constant evaluation and
///   source map, which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert a plain literal, `concat!`,
///   `format!`, and `include_str!` of the configured extension and of another.
/// - witness: `tests::ui_embedded_source`
fn literal<'tcx>(
    tcx: TyCtxt<'tcx>,
    constant: &ConstOperand<'tcx>,
    site: Site,
) -> Maybe<Seed, constant_reading::NotText>
{
    let ty = constant.const_.ty();
    let ty::Ref(_, pointee, _) = *ty.kind()
    else {
        return Maybe::Absent(constant_reading::NotText::OtherType);
    };
    let span = constant.span;
    let expansion = span.ctxt().outer_expn_data();
    if let ExpnKind::Macro(MacroKind::Bang, name) = expansion.kind
        && name == sym::include_str
        && pointee.is_str()
    {
        let snippet = tcx
            .sess
            .source_map()
            .span_to_snippet(expansion.call_site)
            .unwrap_or_default();
        let path = snippet.split('"').nth(1).unwrap_or_default().to_owned();
        let text = match constant_text(tcx, constant) {
            Maybe::Present(text) => text,
            Maybe::Absent(_) => String::new(),
        };
        return Maybe::Present(Seed {
            span: expansion.call_site,
            lint_root: site.lint_root,
            text,
            origin: Origin::Included(path),
        });
    }
    if pointee.is_str() {
        let Maybe::Present(text) = constant_text(tcx, constant)
        else {
            return Maybe::Absent(constant_reading::NotText::Unreadable);
        };
        return Maybe::Present(Seed {
            span: span.source_callsite(),
            lint_root: site.lint_root,
            text,
            origin: Origin::Literal,
        });
    }
    // A `format!` template is a byte string whose encoding is the compiler's
    // own, and its constant carries no span. The macro call the template is
    // handed in spells it as its first string literal, which is read instead.
    let is_bytes = matches!(
        *pointee.kind(),
        ty::Array(element, _) | ty::Slice(element)
            if matches!(*element.kind(), ty::Uint(ty::UintTy::U8))
    );
    if !is_bytes || !site.span.from_expansion() {
        return Maybe::Absent(constant_reading::NotText::OtherType);
    }
    let call = site.span.source_callsite();
    let Ok(snippet) = tcx.sess.source_map().span_to_snippet(call)
    else {
        return Maybe::Absent(constant_reading::NotText::Unreadable);
    };
    let Maybe::Present(range) = first_string_literal(Snippet(&snippet))
    else {
        return Maybe::Absent(constant_reading::NotText::OtherType);
    };
    let text = snippet
        .get(range.start.0 .. range.end.0)
        .unwrap_or_default();
    let (Ok(first), Ok(past)) = (u32::try_from(range.start.0), u32::try_from(range.end.0))
    else {
        return Maybe::Absent(constant_reading::NotText::Unreadable);
    };
    let lo = call.lo();
    Maybe::Present(Seed {
        span: call
            .with_lo(rustc_span::BytePos(lo.0.saturating_add(first)))
            .with_hi(rustc_span::BytePos(lo.0.saturating_add(past))),
        lint_root: site.lint_root,
        text: unescaped(Snippet(text)),
        origin: Origin::Literal,
    })
}

/// The source text of a macro call or of a literal.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
struct Snippet<'text>(&'text str);

/// A byte offset inside a [`Snippet`].
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
struct SnippetIndex(usize);

/// The bytes of a literal inside a [`Snippet`], from its first byte to just
/// past its last.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LiteralRange
{
    /// The literal's first byte, its `r` or its opening quote.
    start: SnippetIndex,
    /// Just past the literal's closing quote and hashes.
    end: SnippetIndex,
}

/// The byte range of the first string literal in a macro call's source text.
///
/// # Specification
/// - ensures: `Present` with the range from the literal's opening `r` or quote
///   to just past its closing quote and hashes, for the first `"` that opens a
///   string literal; a byte string literal, `b"…"`, is passed over; `Absent`
///   when no string literal closes.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures write `format!` with a plain template and an
///   argument.
/// - witness: `tests::ui_embedded_source`
#[spec(ensures: |output| match output {
    Maybe::Present(range) => range.start < range.end && range.end.0 <= snippet.0.len(),
    Maybe::Absent(_) => true,
})]
fn first_string_literal(snippet: Snippet<'_>) -> Maybe<LiteralRange, constant_reading::NotText>
{
    let bytes = snippet.0.as_bytes();
    let mut index = 0_usize;
    while let Some(&byte) = bytes.get(index) {
        if byte != b'"' {
            index = index.saturating_add(1);
            continue;
        }
        // Count the hashes and the `r` before the quote.
        let mut start = index;
        let mut hashes = 0_usize;
        while start > 0 && bytes.get(start.saturating_sub(1)) == Some(&b'#') {
            start = start.saturating_sub(1);
            hashes = hashes.saturating_add(1);
        }
        let raw = start > 0 && bytes.get(start.saturating_sub(1)) == Some(&b'r');
        if raw {
            start = start.saturating_sub(1);
        }
        else {
            start = index;
            hashes = 0;
        }
        let byte_string = start > 0 && bytes.get(start.saturating_sub(1)) == Some(&b'b');
        let mut cursor = index.saturating_add(1);
        let mut end = None;
        while let Some(&inner) = bytes.get(cursor) {
            if !raw && inner == b'\\' {
                cursor = cursor.saturating_add(2);
                continue;
            }
            let closing = inner == b'"'
                && (0 .. hashes).all(|offset| {
                    bytes.get(cursor.saturating_add(offset).saturating_add(1)) == Some(&b'#')
                });
            if closing {
                end = Some(cursor.saturating_add(1).saturating_add(hashes));
                break;
            }
            cursor = cursor.saturating_add(1);
        }
        let Some(end) = end
        else {
            return Maybe::Absent(constant_reading::NotText::Unreadable);
        };
        if !byte_string {
            return Maybe::Present(LiteralRange {
                start: SnippetIndex(start),
                end: SnippetIndex(end),
            });
        }
        index = end;
    }
    Maybe::Absent(constant_reading::NotText::OtherType)
}

/// The text a string constant holds, when its bytes can be read as UTF-8.
///
/// # Specification
/// - ensures: the constant's bytes, as rustc reads them for a diagnostic, when
///   they are valid UTF-8.
/// - panics: none.
/// - executable: none — the outcome is the compiler's constant evaluation.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert literals of one line and of several,
///   and `concat!` of both.
/// - witness: `tests::ui_embedded_source`
fn constant_text<'tcx>(
    tcx: TyCtxt<'tcx>,
    constant: &ConstOperand<'tcx>,
) -> Maybe<String, constant_reading::NotText>
{
    let Const::Val(value, _) = constant.const_
    else {
        return Maybe::Absent(constant_reading::NotText::OtherType);
    };
    value
        .try_get_slice_bytes_for_diagnostics(tcx)
        .and_then(|bytes| core::str::from_utf8(bytes).ok())
        .map_or(
            Maybe::Absent(constant_reading::NotText::Unreadable),
            |text| Maybe::Present(text.to_owned()),
        )
}

/// The text a string literal's source spelling denotes, line breaks decoded.
///
/// # Specification
/// - ensures: for a raw literal, the characters between its quotes; for any
///   other, the characters between its quotes with `\n` read as a line break, a
///   backslash before a line break and the whitespace after it dropped, and
///   every other escape kept as written; for a snippet that is no literal, the
///   snippet.
/// - panics: none.
/// - executable: none — the decoding is the body; a predicate could only repeat
///   it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures write `format!` templates with an escaped
///   line break, with a continued line, and on one line.
/// - witness: `tests::ui_embedded_source`
fn unescaped(snippet: Snippet<'_>) -> String
{
    let raw = snippet.0.starts_with('r');
    let inner = snippet
        .0
        .split_once('"')
        .and_then(|(_, rest)| rest.rsplit_once('"'))
        .map_or(snippet.0, |(inner, _)| inner);
    if raw {
        return inner.to_owned();
    }
    let mut text = String::with_capacity(inner.len());
    let mut characters = inner.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '\\' {
            text.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => text.push('\n'),
            Some('\n') => while characters.next_if(|next| next.is_whitespace()).is_some() {},
            Some(other) => {
                text.push('\\');
                text.push(other);
            },
            None => text.push('\\'),
        }
    }
    text
}

/// How a library call carries what it is handed into what it returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Carry
{
    /// The result is the receiver, or a view of it with the same layout.
    Same,
    /// The result is an element of the receiver, inside an `Option`.
    Optional,
    /// The result is an element of the receiver.
    Element,
    /// The result's elements are the receiver's, each paired after its index.
    Enumerated,
    /// The result is made of everything handed, in a layout the rule does not
    /// know.
    Mixed,
}

/// How a call into the standard library carries its arguments.
///
/// # Specification
/// - ensures: by the callee's name: views and copies are [`Carry::Same`];
///   `next`, `first`, `last`, `get`, `nth`, `pop`, `next_back` and `peek` are
///   [`Carry::Optional`]; `index` is [`Carry::Element`]; `enumerate` is
///   [`Carry::Enumerated`]; every other callee is [`Carry::Mixed`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures iterate case tables by `for`, by `iter`, by
///   `enumerate` and by index, and read one field of each case.
/// - witness: `tests::ui_embedded_source`
#[spec(ensures: |output| matches!(output, Carry::Same | Carry::Optional | Carry::Element | Carry::Enumerated | Carry::Mixed))]
fn carry(
    tcx: TyCtxt<'_>,
    callee: DefId,
) -> Carry
{
    match tcx.item_name(callee).as_str() {
        "clone" | "to_owned" | "as_ref" | "borrow" | "deref" | "as_slice" | "as_str" | "to_vec"
        | "into_iter" | "iter" | "iter_mut" | "to_string" | "into" | "from" | "copied"
        | "cloned" | "by_ref" | "as_mut_slice" | "into_boxed_slice" => Carry::Same,
        "next" | "first" | "last" | "get" | "nth" | "pop" | "next_back" | "peek" => Carry::Optional,
        "index" => Carry::Element,
        "enumerate" => Carry::Enumerated,
        _ => Carry::Mixed,
    }
}

/// Whether a type is one of the configured types.
///
/// # Specification
/// - ensures: answers [`Sink::Sink`] exactly when `ty` is an ADT whose
///   definition is in `types`.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert into the configured type and into
///   another wrapper of the same shape, which is not a sink.
/// - witness: `tests::ui_embedded_source`
#[spec(ensures: |output| (output == Sink::Sink) == matches!(*ty.kind(), ty::Adt(definition, _) if types.contains(&definition.did())))]
fn sink_type(
    types: &HashSet<DefId>,
    ty: Ty<'_>,
) -> Sink
{
    if let ty::Adt(definition, _) = *ty.kind()
        && types.contains(&definition.did())
    {
        Sink::Sink
    }
    else {
        Sink::Other
    }
}

/// Whether a call or a type converts into a configured type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Sink
{
    /// It does.
    Sink,
    /// It does not.
    Other,
}

/// Whether a call converts its first argument into a configured type.
///
/// # Specification
/// - ensures: [`Sink::Sink`] for `From::from` whose `Self` is configured, for
///   `Into::into` whose target is configured, and for the constructor function
///   of a configured structure; [`Sink::Other`] otherwise.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert by `From::from`, by `.into()` in a
///   generic helper, by a constructor named as a function, and by a `From` impl
///   of another type, which is not a sink.
/// - witness: `tests::ui_embedded_source`
#[spec(ensures: |output| output == Sink::Other || !types.is_empty())]
fn sink_call<'tcx>(
    tcx: TyCtxt<'tcx>,
    types: &HashSet<DefId>,
    callee: DefId,
    args: GenericArgsRef<'tcx>,
) -> Sink
{
    let conversion = tcx
        .trait_of_assoc(callee)
        .and_then(|trait_id| tcx.get_diagnostic_name(trait_id));
    if conversion == Some(sym::From) && tcx.item_name(callee) == sym::from {
        return args
            .types()
            .next()
            .map_or(Sink::Other, |target| sink_type(types, target));
    }
    if conversion == Some(sym::Into) && tcx.item_name(callee).as_str() == "into" {
        return args
            .types()
            .nth(1)
            .map_or(Sink::Other, |target| sink_type(types, target));
    }
    if tcx.def_kind(callee) == DefKind::Ctor(CtorOf::Struct, CtorKind::Fn)
        && let Some(parent) = tcx.opt_parent(callee)
        && types.contains(&parent)
    {
        return Sink::Sink;
    }
    Sink::Other
}

/// The facts a place holds once its projection is read.
///
/// # Specification
/// - ensures: the local's facts read through each field and element step of the
///   projection, a dereference, a downcast and a cast reading nothing.
/// - panics: none.
/// - executable: none — the outcome is a filter over the local's facts; a
///   predicate could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures read fields of tuples, elements of arrays
///   and payloads of `Option`.
/// - witness: `tests::ui_embedded_source`
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`ProjectionElem` is rustc's MIR projection, whose variants move with the pinned \
              nightly; only a field and an index step into a value"
)]
fn place_facts(
    facts: &Facts,
    place: Place<'_>,
) -> BTreeSet<Fact>
{
    let Place { local, projection } = place;
    let mut held = facts.of(local);
    for element in projection {
        let step = match element {
            ProjectionElem::Field(index, _) => Step::Field(index.as_u32()),
            ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } => Step::Element,
            _ => continue,
        };
        held = held
            .into_iter()
            .filter_map(|fact| match fact.read(step) {
                Maybe::Present(read) => Some(read),
                Maybe::Absent(_) => None,
            })
            .collect();
    }
    held
}

/// The path an assignment's destination projection writes into.
///
/// # Specification
/// - ensures: the field and element steps of the projection, outermost first,
///   at most [`DEPTH`] of them.
/// - panics: none.
/// - executable: none — the outcome is a fold over the projection; a predicate
///   could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures write a literal into a field of a local
///   tuple before converting the tuple's field.
/// - witness: `tests::ui_embedded_source`
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "`ProjectionElem` is rustc's MIR projection, whose variants move with the pinned \
              nightly; only a field and an index step into a value"
)]
fn written_path(place: Place<'_>) -> Path
{
    place
        .projection
        .iter()
        .filter_map(|element| match element {
            ProjectionElem::Field(index, _) => Some(Step::Field(index.as_u32())),
            ProjectionElem::Index(_) | ProjectionElem::ConstantIndex { .. } => Some(Step::Element),
            _ => None,
        })
        .fold(Path::WHOLE, Path::then)
}

/// Facts placed at `path` inside the value they move into.
///
/// # Specification
/// - ensures: each fact's position is `path` followed by its own, innermost
///   steps dropped past [`DEPTH`].
/// - panics: none.
/// - executable: none — the outcome is a map over the facts; a predicate could
///   only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures build tuples and arrays of literals.
/// - witness: `tests::ui_embedded_source`
fn placed(
    facts: BTreeSet<Fact>,
    path: Path,
) -> BTreeSet<Fact>
{
    facts
        .into_iter()
        .map(|fact| Fact {
            at: path
                .steps()
                .iter()
                .rev()
                .fold(fact.at, |at, &step| at.inside(step)),
            ..fact
        })
        .collect()
}

/// The facts an operand holds.
///
/// # Specification
/// - ensures: a copied or moved place holds [`place_facts`]; a constant holds
///   [`constant_facts`] when the body is read for its literals and nothing
///   otherwise.
/// - panics: none.
/// - executable: none — the outcome is [`place_facts`] or [`constant_facts`].
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert literals directly and through
///   locals.
/// - witness: `tests::ui_embedded_source`
fn operand_facts<'tcx>(
    tcx: TyCtxt<'tcx>,
    facts: &Facts,
    operand: &Operand<'tcx>,
    reading: Reading,
    site: Site,
    seeds: &mut Seeds,
) -> BTreeSet<Fact>
{
    match *operand {
        Operand::Copy(place) | Operand::Move(place) => place_facts(facts, place),
        Operand::Constant(ref constant) if reading == Reading::Literals => {
            constant_facts(tcx, constant, site, seeds)
        },
        Operand::Constant(_) | Operand::RuntimeChecks(_) => BTreeSet::new(),
    }
}

/// Where a statement or terminator stands: the node whose lint level it
/// takes, and its span.
#[derive(Clone, Copy, Debug)]
struct Site
{
    /// The node whose lint level a finding there takes.
    lint_root: HirId,
    /// The statement's or terminator's span, inside a macro when one wrote it.
    span: Span,
}

/// Where a statement or terminator stands.
///
/// # Specification
/// - ensures: [`lint_root`] of `source_info`, and its span.
/// - panics: none.
/// - executable: none — the outcome is [`lint_root`] and a field read.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a `format!` template, which only the
///   call's span locates.
/// - witness: `tests::ui_embedded_source`
fn site_of(
    tcx: TyCtxt<'_>,
    body: &Body<'_>,
    source_info: SourceInfo,
) -> Site
{
    Site {
        lint_root: lint_root(tcx, body, source_info),
        span: source_info.span,
    }
}

/// The node whose lint level a statement or terminator takes.
///
/// # Specification
/// - ensures: the scope's lint root, or the body's owner when the scope was
///   inlined from elsewhere.
/// - panics: none.
/// - executable: none — the outcome is the compiler's source scopes, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures allow the rule on one test function and
///   refuse the same literal in another.
/// - witness: `tests::ui_embedded_source`
fn lint_root(
    tcx: TyCtxt<'_>,
    body: &Body<'_>,
    source_info: SourceInfo,
) -> HirId
{
    source_info
        .scope
        .lint_root(&body.source_scopes)
        .unwrap_or_else(|| {
            body.source
                .def_id()
                .as_local()
                .map_or(rustc_hir::CRATE_HIR_ID, |owner| {
                    tcx.local_def_id_to_hir_id(owner)
                })
        })
}

/// The locals each local refers to, and where inside them, by its borrows.
///
/// # Specification
/// - ensures: a local assigned a borrow or a raw pointer of a place refers to
///   that place's local at the place's field and element steps.
/// - panics: none.
/// - executable: none — the outcome is a filter over the body's statements.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures push a literal into a `String` through
///   `&mut` before converting it.
/// - witness: `tests::ui_embedded_source`
fn referents(body: &Body<'_>) -> HashMap<Local, Vec<(Local, Path)>>
{
    let mut referred: HashMap<Local, Vec<(Local, Path)>> = HashMap::new();
    for statement in body
        .basic_blocks
        .iter()
        .flat_map(|block| block.statements.iter())
    {
        let StatementKind::Assign(ref assignment) = statement.kind
        else {
            continue;
        };
        let (destination, ref rvalue) = **assignment;
        if let Rvalue::Ref(_, _, borrowed) | Rvalue::RawPtr(_, borrowed) = *rvalue {
            let Place { local: written, .. } = destination;
            let Place { local: lent, .. } = borrowed;
            referred
                .entry(written)
                .or_default()
                .push((lent, written_path(borrowed)));
        }
    }
    referred
}

/// Settle what every local of a body holds.
///
/// # Specification
/// - ensures: in [`Reading::Parameters`] each parameter holds itself at the
///   whole value; an assignment's destination holds what its operands hold,
///   placed at the destination's projection and at each field or element an
///   aggregate puts them; a write through a borrow of a local of the body
///   reaches that local; a call's destination holds what [`carry`] gives for a
///   standard-library callee and everything its arguments hold otherwise, a
///   conversion into a configured type holding nothing; a call handed a mutable
///   borrow of a local of the body writes what the other arguments hold into
///   that local. Repeats until nothing grows.
/// - panics: none.
/// - executable: none — the outcome is a dataflow fixpoint over the body's MIR;
///   a predicate could only recompute it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures carry literals through locals, loops,
///   tuples, arrays, iterators, `format!` and `push_str` before a conversion.
/// - witness: `tests::ui_embedded_source`
fn settle<'tcx>(
    tcx: TyCtxt<'tcx>,
    types: &HashSet<DefId>,
    body: &Body<'tcx>,
    reading: Reading,
    seeds: &mut Seeds,
) -> Facts
{
    let mut facts = Facts {
        locals: vec![BTreeSet::new(); body.local_decls.len()],
        handed: HashMap::new(),
    };
    if reading == Reading::Parameters {
        for (index, local) in body.args_iter().enumerate() {
            facts.extend(
                local,
                BTreeSet::from([Fact {
                    held: Held::Parameter {
                        parameter: Parameter(index),
                        from: Path::WHOLE,
                    },
                    at: Path::WHOLE,
                }]),
            );
        }
    }
    let referred = referents(body);
    let arguments: HashSet<Local> = body.args_iter().collect();
    // A write through a borrow reaches the borrowed local of this body; one
    // through a parameter's borrow stays inside the function.
    let write = |facts: &mut Facts, destination: Place<'tcx>, held: BTreeSet<Fact>| -> Grew {
        let Place { local: written, .. } = destination;
        let mut grew = facts.extend(written, placed(held.clone(), written_path(destination)));
        if destination.is_indirect_first_projection()
            && !arguments.contains(&written)
            && let Some(targets) = referred.get(&written)
        {
            for &(target, at) in targets {
                if facts.extend(target, placed(held.clone(), at)) == Grew::Grew {
                    grew = Grew::Grew;
                }
            }
        }
        grew
    };
    loop {
        let mut grew = Grew::Same;
        for (block_index, block) in body.basic_blocks.iter_enumerated() {
            for (statement_index, statement) in block.statements.iter().enumerate() {
                let StatementKind::Assign(ref assignment) = statement.kind
                else {
                    continue;
                };
                let root = site_of(tcx, body, statement.source_info);
                let (destination, ref rvalue) = **assignment;
                let held = match *rvalue {
                    Rvalue::Use(ref operand, _) | Rvalue::Cast(_, ref operand, _) => {
                        operand_facts(tcx, &facts, operand, reading, root, seeds)
                    },
                    Rvalue::Repeat(ref operand, _) => placed(
                        operand_facts(tcx, &facts, operand, reading, root, seeds),
                        Path::WHOLE.then(Step::Element),
                    ),
                    Rvalue::Ref(_, _, borrowed)
                    | Rvalue::RawPtr(_, borrowed)
                    | Rvalue::CopyForDeref(borrowed) => place_facts(&facts, borrowed),
                    Rvalue::Aggregate(ref kind, ref operands) => {
                        let configured = if let AggregateKind::Adt(def_id, ..) = **kind
                            && types.contains(&def_id)
                        {
                            Sink::Sink
                        }
                        else {
                            Sink::Other
                        };
                        if configured == Sink::Sink {
                            let converted = operands
                                .iter()
                                .map(|operand| {
                                    operand_facts(tcx, &facts, operand, reading, root, seeds)
                                })
                                .collect();
                            facts.handed.insert(
                                rustc_middle::mir::Location {
                                    block: block_index,
                                    statement_index,
                                },
                                converted,
                            );
                            BTreeSet::new()
                        }
                        else {
                            let array = matches!(**kind, AggregateKind::Array(_));
                            let mut held = BTreeSet::new();
                            for (index, operand) in operands.iter_enumerated() {
                                let step = if array {
                                    Step::Element
                                }
                                else {
                                    Step::Field(index.as_u32())
                                };
                                held.extend(placed(
                                    operand_facts(tcx, &facts, operand, reading, root, seeds),
                                    Path::WHOLE.then(step),
                                ));
                            }
                            held
                        }
                    },
                    Rvalue::ThreadLocalRef(_)
                    | Rvalue::BinaryOp(..)
                    | Rvalue::UnaryOp(..)
                    | Rvalue::Discriminant(_)
                    | Rvalue::WrapUnsafeBinder(..)
                    | Rvalue::Reborrow(..) => BTreeSet::new(),
                };
                if write(&mut facts, destination, held) == Grew::Grew {
                    grew = Grew::Grew;
                }
            }
            let Some(terminator) = block.terminator.as_ref()
            else {
                continue;
            };
            let TerminatorKind::Call {
                ref func,
                ref args,
                destination,
                ..
            } = terminator.kind
            else {
                continue;
            };
            let root = site_of(tcx, body, terminator.source_info);
            let handed: Vec<BTreeSet<Fact>> = args
                .iter()
                .map(|argument| operand_facts(tcx, &facts, &argument.node, reading, root, seeds))
                .collect();
            let location = body.terminator_loc(block_index);
            let callee = func.const_fn_def();
            if callee.is_some_and(|(callee, callee_args)| {
                sink_call(tcx, types, callee, callee_args) == Sink::Sink
            }) {
                facts.handed.insert(location, handed);
                continue;
            }
            let library = callee.is_some_and(|(callee, _)| {
                LIBRARY_CRATES.contains(&tcx.crate_name(callee.krate).as_str())
            });
            let carried = callee.map_or(Carry::Mixed, |(callee, _)| {
                if library {
                    carry(tcx, callee)
                }
                else {
                    Carry::Mixed
                }
            });
            let receiver = handed.first().into_iter().flatten().copied();
            let returned: BTreeSet<Fact> = match carried {
                Carry::Same => receiver.collect(),
                Carry::Optional | Carry::Element => receiver
                    .filter_map(|fact| match fact.read(Step::Element) {
                        Maybe::Present(read) => Some(read),
                        Maybe::Absent(_) => None,
                    })
                    .map(|fact| {
                        if carried == Carry::Optional {
                            Fact {
                                at: fact.at.inside(Step::Field(0)),
                                ..fact
                            }
                        }
                        else {
                            fact
                        }
                    })
                    .collect(),
                Carry::Enumerated => receiver
                    .filter_map(|fact| match fact.read(Step::Element) {
                        Maybe::Present(read) => Some(Fact {
                            at: read.at.inside(Step::Field(1)).inside(Step::Element),
                            ..read
                        }),
                        Maybe::Absent(_) => None,
                    })
                    .collect(),
                Carry::Mixed => handed.iter().flatten().map(|fact| fact.mixed()).collect(),
            };
            if carried == Carry::Mixed && !returned.is_empty() {
                // A mutable borrow handed to a call may be written with what
                // the arguments hold.
                for argument in &**args {
                    let (Operand::Copy(place) | Operand::Move(place)) = argument.node
                    else {
                        continue;
                    };
                    let ty::Ref(_, _, ty::Mutability::Mut) =
                        *place.ty(&body.local_decls, tcx).ty.kind()
                    else {
                        continue;
                    };
                    let Place { local: lent, .. } = place;
                    if arguments.contains(&lent) {
                        continue;
                    }
                    if let Some(targets) = referred.get(&lent) {
                        for &(target, at) in targets {
                            if facts.extend(target, placed(returned.clone(), at)) == Grew::Grew {
                                grew = Grew::Grew;
                            }
                        }
                    }
                }
            }
            if write(&mut facts, destination, returned) == Grew::Grew {
                grew = Grew::Grew;
            }
            facts.handed.insert(location, handed);
        }
        if grew == Grew::Same {
            return facts;
        }
    }
}

/// The body a call reaches, when it is one a summary reads.
///
/// # Specification
/// - ensures: `Present` with the resolved item when the callee resolves to a
///   function or closure outside the standard library whose MIR is available; a
///   call of a closure through `Fn`, `FnMut` or `FnOnce` names the closure;
///   `Absent` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's instance resolution.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a local helper, a dependency's helper,
///   a closure, a trait method on a type parameter and a standard-library
///   function.
/// - witness: `tests::ui_embedded_source`
fn followed<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    callee: DefId,
    args: GenericArgsRef<'tcx>,
) -> Maybe<DefId, callee_reading::Unfollowed>
{
    if let Some(trait_id) = tcx.trait_of_assoc(callee)
        && tcx.fn_trait_kind_from_def_id(trait_id).is_some()
        && let Some(ty::Closure(closure, _)) = args.types().next().map(|held| *held.kind())
    {
        return Maybe::Present(closure);
    }
    if !matches!(tcx.def_kind(callee), DefKind::Fn | DefKind::AssocFn)
        || tcx.generics_of(callee).count() != args.len()
    {
        return Maybe::Absent(callee_reading::Unfollowed::NotAnItem);
    }
    let args = tcx.erase_and_anonymize_regions(args);
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`InstanceKind` is rustc's instance kind, whose variants move with the pinned \
                  nightly; only an item names a body"
    )]
    let target = match Instance::try_resolve(tcx, typing_env, callee, args) {
        Ok(Some(instance)) => match instance.def {
            InstanceKind::Item(target) => target,
            _ => return Maybe::Absent(callee_reading::Unfollowed::NoBody),
        },
        Ok(None) | Err(_) => return Maybe::Absent(callee_reading::Unfollowed::NoBody),
    };
    if LIBRARY_CRATES.contains(&tcx.crate_name(target.krate).as_str()) {
        return Maybe::Absent(callee_reading::Unfollowed::Library);
    }
    if tcx.is_mir_available(target) {
        Maybe::Present(target)
    }
    else {
        Maybe::Absent(callee_reading::Unfollowed::NoBody)
    }
}

/// The facts `summary` says reach a sink, among those `parameter` is handed.
///
/// # Specification
/// - ensures: each handed fact whose position overlaps a summarized position of
///   the parameter, with the witness; a parameter's part moves to the part the
///   callee reads.
/// - panics: none.
/// - executable: none — the outcome is a filter over the handed facts; a
///   predicate could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures hand helpers tuples whose one field the
///   helper converts.
/// - witness: `tests::ui_embedded_source`
fn reached(
    hits: &mut Vec<(Fact, Witness)>,
    summary: &Summary,
    parameter: Parameter,
    handed: impl IntoIterator<Item = Fact>,
)
{
    let Some(&(ref paths, witness)) = summary.0.get(&parameter)
    else {
        return;
    };
    for fact in handed {
        for &path in paths {
            if fact.at.overlap(path) == Overlap::Apart {
                continue;
            }
            let held = match fact.held {
                Held::Literal(_) => fact.held,
                Held::Parameter { parameter, from } => Held::Parameter {
                    parameter,
                    from: path.beyond(fact.at, from),
                },
            };
            hits.push((Fact { held, ..fact }, witness));
        }
    }
}

/// The sinks a settled body reaches, directly and through summarized calls.
///
/// # Specification
/// - ensures: [`Scan::Needs`] lists the followed callees whose summaries are
///   missing and not themselves being read; otherwise [`Scan::Ready`] holds
///   each fact a conversion into a configured type receives, each fact a
///   summarized callee says reaches a sink, the facts of a closure's captures
///   its summary reads, and every fact handed beside a closure whose own
///   parameters reach a sink.
/// - panics: none.
/// - executable: none — the outcome is a scan of the body's calls against the
///   summaries; a predicate could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert directly, through local and foreign
///   helpers, through closures called and handed to an iterator.
/// - witness: `tests::ui_embedded_source`
fn scan<'tcx>(
    tcx: TyCtxt<'tcx>,
    types: &HashSet<DefId>,
    body: &Body<'tcx>,
    owner: DefId,
    facts: &Facts,
    done: &HashMap<DefId, Summary>,
    reading: &HashSet<DefId>,
) -> Scan
{
    let typing_env = TypingEnv::post_analysis(tcx, owner);
    let mut needs = Vec::new();
    let mut hits = Vec::new();
    let empty = Summary::default();
    let mut lookup = |target: DefId| -> Maybe<&Summary, callee_reading::Unfollowed> {
        if let Some(summary) = done.get(&target) {
            return Maybe::Present(summary);
        }
        if reading.contains(&target) {
            return Maybe::Present(&empty);
        }
        needs.push(target);
        Maybe::Absent(callee_reading::Unfollowed::NoBody)
    };
    for (block_index, block) in body.basic_blocks.iter_enumerated() {
        for (statement_index, statement) in block.statements.iter().enumerate() {
            let StatementKind::Assign(ref assignment) = statement.kind
            else {
                continue;
            };
            let Rvalue::Aggregate(ref kind, _) = assignment.1
            else {
                continue;
            };
            let AggregateKind::Adt(def_id, ..) = **kind
            else {
                continue;
            };
            if !types.contains(&def_id) {
                continue;
            }
            let witness = Witness {
                span: statement.source_info.span,
                owner,
            };
            let location = rustc_middle::mir::Location {
                block: block_index,
                statement_index,
            };
            for held in facts.handed_at(location) {
                hits.extend(held.iter().map(|&fact| (fact, witness)));
            }
        }
        let Some(terminator) = block.terminator.as_ref()
        else {
            continue;
        };
        let TerminatorKind::Call {
            ref func,
            ref args,
            fn_span,
            ..
        } = terminator.kind
        else {
            continue;
        };
        let handed = facts.handed_at(body.terminator_loc(block_index));
        if handed.iter().all(BTreeSet::is_empty) {
            continue;
        }
        let Some((callee, callee_args)) = func.const_fn_def()
        else {
            continue;
        };
        let witness = Witness {
            span: fn_span,
            owner,
        };
        if sink_call(tcx, types, callee, callee_args) == Sink::Sink {
            if let Some(converted) = handed.first() {
                hits.extend(converted.iter().map(|&fact| (fact, witness)));
            }
            continue;
        }
        // A closure handed to a call beside a value is taken to receive it.
        for (index, argument) in args.iter().enumerate() {
            let argument_ty = argument.node.ty(&body.local_decls, tcx).peel_refs();
            let ty::Closure(closure, _) = *argument_ty.kind()
            else {
                continue;
            };
            let Maybe::Present(summary) = lookup(closure)
            else {
                continue;
            };
            if let Some(captured) = handed.get(index) {
                reached(&mut hits, summary, Parameter(0), captured.iter().copied());
            }
            let Some(&(_, witness)) = summary
                .0
                .iter()
                .find(|&(&parameter, _)| parameter != Parameter(0))
                .map(|(_, reach)| reach)
            else {
                continue;
            };
            for (other, held) in handed.iter().enumerate() {
                if other != index {
                    hits.extend(held.iter().map(|&fact| (fact, witness)));
                }
            }
        }
        let Maybe::Present(target) = followed(tcx, typing_env, callee, callee_args)
        else {
            continue;
        };
        let Maybe::Present(summary) = lookup(target)
        else {
            continue;
        };
        let closure_call = tcx.is_closure_like(target)
            && tcx
                .trait_of_assoc(callee)
                .is_some_and(|trait_id| tcx.fn_trait_kind_from_def_id(trait_id).is_some());
        if closure_call {
            // `Fn::call(&closure, (a, b))`: the closure's captures, then each
            // field of the argument tuple as one parameter.
            if let Some(captured) = handed.first() {
                reached(&mut hits, summary, Parameter(0), captured.iter().copied());
            }
            let Some(tupled) = handed.get(1)
            else {
                continue;
            };
            for &parameter in summary.0.keys() {
                let Some(field) = parameter.0.checked_sub(1)
                else {
                    continue;
                };
                let Ok(field) = u32::try_from(field)
                else {
                    continue;
                };
                let inside = tupled
                    .iter()
                    .filter_map(|fact| match fact.read(Step::Field(field)) {
                        Maybe::Present(read) => Some(read),
                        Maybe::Absent(_) => None,
                    });
                reached(&mut hits, summary, parameter, inside);
            }
            continue;
        }
        for (index, held) in handed.iter().enumerate() {
            reached(&mut hits, summary, Parameter(index), held.iter().copied());
        }
    }
    if needs.is_empty() {
        Scan::Ready(hits)
    }
    else {
        let mut seen = HashSet::new();
        needs.retain(|callee| seen.insert(*callee));
        Scan::Needs(needs)
    }
}

/// Whether a text runs over more than one line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Lines
{
    /// No line break before the last character.
    One,
    /// A line break before the last character.
    Several,
}

/// How many lines a text runs over, a last line break aside.
///
/// # Specification
/// - ensures: [`Lines::Several`] exactly when a line break stands before the
///   text's last character.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures convert a literal of one line, one ending in
///   a line break, and one of two lines.
/// - witness: `tests::ui_embedded_source`
#[spec(ensures: |output| (output == Lines::Several) == text.text.strip_suffix('\n').unwrap_or(&text.text).contains('\n'))]
fn lines(text: &Seed) -> Lines
{
    let body = text.text.strip_suffix('\n').unwrap_or(&text.text);
    if body.contains('\n') {
        Lines::Several
    }
    else {
        Lines::One
    }
}

impl<'tcx> LateLintPass<'tcx> for EmbeddedSource
{
    /// Refuse each multi-line literal that reaches a configured type, list
    /// every inline source that does, and record which bodies reach one.
    ///
    /// # Specification
    /// - ensures: when configured, reports the configuration's defects at the
    ///   crate root; a unit another crate can depend on writes its reach
    ///   record, blind when no configured type resolves; reads each function
    ///   and closure body of the crate that reaches a sink for its literals,
    ///   and the constants of every body; refuses, once per literal, each
    ///   literal of two or more lines and each `include_str!` of another
    ///   extension that reaches a sink, unless the package is allowed; writes
    ///   the reached inline literals to the list directory when one is
    ///   configured.
    /// - panics: none.
    /// - executable: none — the outcome is emitted diagnostics and written
    ///   files.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the refused fixture holds each refused form, the
    ///   accepted fixture each accepted form, the list the accepted fixture
    ///   writes is read back, and the fixtures read their dependency's record.
    /// - witness: `tests::ui_embedded_source`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let Maybe::Present(ref configured) = self.configured
        else {
            return;
        };
        report_defects(cx, EMBEDDED_SOURCE, &configured.defects);
        let tcx = cx.tcx;
        let library = !tcx.sess.is_test_crate()
            && tcx
                .crate_types()
                .iter()
                .any(|kind| matches!(*kind, CrateType::Rlib | CrateType::Dylib));
        if configured.types.is_empty() {
            // No body of a crate that cannot name a configured type reaches
            // one, and its calls go only to crates that cannot name one.
            if library {
                write_record(tcx, &configured.heading, &Record::Blind);
            }
            return;
        }
        let types = &configured.types;
        let mut reaching = Reaching::new(configured.heading.clone());
        let mut sinking: HashSet<DefIndex> = HashSet::new();
        let mut seeds = Seeds::default();
        let mut summaries = Summaries::default();
        let mut reached: BTreeMap<SeedId, Witness> = BTreeMap::new();
        for owner in tcx.hir_body_owners() {
            if !matches!(
                tcx.def_kind(owner),
                DefKind::Fn | DefKind::AssocFn | DefKind::Closure
            ) || !tcx.is_mir_available(owner.to_def_id())
            {
                continue;
            }
            let def_id = owner.to_def_id();
            let reach = reaching.reaches(tcx, types, def_id);
            if library && reach == Sink::Sink {
                sinking.insert(def_id.index);
            }
            let body = tcx.optimized_mir(def_id);
            // The pass reads the constants of every body, so each constant
            // takes its seed in the order the bodies name it.
            read_constants(tcx, types, body, &mut seeds);
            // A body that reaches no sink holds no literal that reaches one.
            if reach == Sink::Other {
                continue;
            }
            let facts = settle(tcx, types, body, Reading::Literals, &mut seeds);
            let hits = loop {
                match scan(
                    tcx,
                    types,
                    body,
                    def_id,
                    &facts,
                    &summaries.0,
                    &HashSet::new(),
                ) {
                    Scan::Ready(hits) => break hits,
                    Scan::Needs(callees) => {
                        for callee in callees {
                            summaries.compute(tcx, types, &mut reaching, callee);
                        }
                    },
                }
            };
            for (fact, witness) in hits {
                if let Held::Literal(seed) = fact.held {
                    reached.entry(seed).or_insert(witness);
                }
            }
        }
        if library {
            write_record(tcx, &configured.heading, &Record::Sinking(sinking));
        }
        if let Maybe::Present(ref directory) = configured.directory {
            write_list(tcx, directory, &seeds, &reached);
        }
        if configured.allowance == Allowance::Allowed {
            return;
        }
        let source_map = tcx.sess.source_map();
        let suffix = format!(".{}", configured.extension);
        for (&seed, witness) in &reached {
            let Some(found) = seeds.table.get(seed.0)
            else {
                continue;
            };
            if source_map.is_imported(found.span) {
                continue;
            }
            let message = match found.origin {
                Origin::Included(ref path) if !path.ends_with(suffix.as_str()) => {
                    format!(
                        "this source is read from `{path}`, which is no `.{}` file",
                        configured.extension
                    )
                },
                Origin::Included(_) => continue,
                Origin::Literal if lines(found) == Lines::One => continue,
                Origin::Literal => {
                    "this source of more than one line is spelled as a Rust literal".to_owned()
                },
            };
            let extension = configured.extension.as_str();
            span_lint_hir_and_then(
                cx,
                EMBEDDED_SOURCE,
                found.lint_root,
                found.span,
                message,
                |diag| {
                    if witness.owner.is_local() {
                        diag.span_note(
                            witness.span,
                            "the source reaches the type a parser reads here",
                        );
                    }
                    else {
                        diag.note(format!(
                            "the source reaches the type a parser reads in `{}`",
                            tcx.def_path_str(witness.owner)
                        ));
                    }
                    diag.help(format!(
                        "write the source in a `.{extension}` file beside the test, and read it \
                         with `include_str!`"
                    ));
                },
            );
        }
    }
}

/// Write the inline sources that reach a sink to the list directory.
///
/// # Specification
/// - ensures: one JSON line per reached literal of the local crate, in seed
///   order, with its file, its byte range in the file, its line and column and
///   its text, written whole to a temporary name in `directory` and renamed
///   over the unit's list, so a reader never meets half a list; a failure is
///   reported as a compiler error and leaves the earlier list.
/// - panics: none.
/// - executable: none — the outcome is a written file.
///
/// # Adequacy
/// - hypothesis: L3 — the accepted fixture's list holds its one-line literal
///   and no fixture read with `include_str!`.
/// - witness: `tests::ui_embedded_source`
fn write_list(
    tcx: TyCtxt<'_>,
    directory: &std::path::Path,
    seeds: &Seeds,
    reached: &BTreeMap<SeedId, Witness>,
)
{
    let source_map = tcx.sess.source_map();
    let mut contents = Vec::new();
    for &seed in reached.keys() {
        let Some(found) = seeds.table.get(seed.0)
        else {
            continue;
        };
        if found.origin != Origin::Literal || source_map.is_imported(found.span) {
            continue;
        }
        let start = source_map.lookup_char_pos(found.span.lo());
        let rustc_span::FileName::Real(ref real) = start.file.name
        else {
            continue;
        };
        let Some(file) = real.local_path()
        else {
            continue;
        };
        let offset =
            |position: rustc_span::BytePos| position.0.saturating_sub(start.file.start_pos.0);
        let record = serde_json::json!({
            "file": file,
            "start": offset(found.span.lo()),
            "end": offset(found.span.hi()),
            "line": start.line,
            "column": start.col.0.saturating_add(1_usize),
            "text": found.text,
        });
        let written = writeln!(contents, "{record}");
        if written.is_err() {
            return;
        }
    }
    let name = crate::evidence::record_name(tcx);
    let path = directory.join(&name);
    // Two compilations of one unit, as two test fixtures sharing a dependency
    // run, stage under names of their own, so neither renames the other's.
    let staged = directory.join(format!("{name}.{}.partial", std::process::id()));
    let written = std::fs::create_dir_all(directory)
        .and_then(|()| std::fs::write(&staged, &contents))
        .and_then(|()| std::fs::rename(&staged, &path));
    if let Err(error) = written {
        tcx.dcx().err(format!(
            "the list of inline sources cannot be written to `{}`: {error}",
            path.display()
        ));
    }
}

/// Write the unit's reach record beside its metadata, so a later unit decides
/// the reach of this crate's bodies without reading them.
///
/// # Specification
/// - ensures: the record format, the crate's hash and `heading`, one on a line,
///   then `blind` for [`Record::Blind`], or `sinking` and each listed body
///   index in ascending order, one on a line; written whole to a temporary name
///   and renamed beside the unit's metadata, so a reader never meets half a
///   record; a unit whose metadata goes to standard output writes none; a
///   failure is reported as a compiler error.
/// - panics: none.
/// - executable: none — the outcome is a written file.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures' dependency writes the record the fixtures
///   read, and a record that leaves its helper out loses a refusal.
/// - witness: `tests::ui_embedded_source`
fn write_record(
    tcx: TyCtxt<'_>,
    heading: &Heading,
    record: &Record,
)
{
    let OutFileName::Real(metadata) = tcx.output_filenames(()).path(OutputType::Metadata)
    else {
        return;
    };
    let path = metadata.with_extension(RECORD_EXTENSION);
    let mut contents = Vec::new();
    let mut written = writeln!(
        contents,
        "{RECORD_FORMAT}\ncrate {:032x}\n{}",
        tcx.crate_hash(rustc_hir::def_id::LOCAL_CRATE).as_u128(),
        heading.0
    );
    match *record {
        Record::Blind => {
            written = written.and_then(|()| writeln!(contents, "blind"));
        },
        Record::Sinking(ref indices) => {
            written = written.and_then(|()| writeln!(contents, "sinking"));
            let mut sorted: Vec<u32> = indices.iter().map(|index| index.as_u32()).collect();
            sorted.sort_unstable();
            for index in sorted {
                written = written.and_then(|()| writeln!(contents, "{index}"));
            }
        },
    }
    if written.is_err() {
        return;
    }
    // Two compilations of one unit stage under names of their own, so neither
    // renames the other's.
    let staged =
        metadata.with_extension(format!("{RECORD_EXTENSION}.{}.partial", std::process::id()));
    let renamed = std::fs::write(&staged, &contents).and_then(|()| std::fs::rename(&staged, &path));
    if let Err(error) = renamed {
        tcx.dcx().err(format!(
            "the reach record cannot be written to `{}`: {error}",
            path.display()
        ));
    }
}
