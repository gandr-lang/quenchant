//! Refuse an allocation reachable from a function marked allocation-free in
//! steady state, and, in a crate whose default turns round, an allocation
//! that its function does not declare.
//!
//! A function opts in with a `# Cost` section in its rustdoc carrying the line
//! `- allocation: none in steady state` (a trailing period allowed). A trait
//! method's declaration may carry it too; every implementation is then held
//! to it, and a call of the method on a type parameter is trusted.
//!
//! The reading is over MIR, per instance: each call in the marked body is
//! resolved under the body's own generic arguments, and its callee's body is
//! read in turn, into `std` and other crates wherever the build holds their
//! MIR. A path reaches an allocation when it calls one of the allocator's
//! entry points, calls a body the build cannot read whose signature returns
//! or writes through `&mut` a value owning heap memory, or calls through a
//! trait object, a function pointer or an unmarked trait method of a type
//! parameter. Summaries are kept per instance for the whole crate, so each
//! body is read once.
//!
//! In a crate that `allocation_default_crates` names, the default turns
//! round: each function is held, and one that allocates declares it as
//! `- allocation: <what>, <why>`. [`AllocationFreeRegion`] states the default
//! whole: the five whys, the origin rule, the exceptions, the base records
//! and the count.
//!
//! # Boundaries
//!
//! - A non-generic call into `core` never allocates and is not followed; a
//!   generic one is, since a closure or an implementation it calls may.
//! - A call that returns `!` ends the steady state and is not followed.
//! - Destructors are not followed: dropping a value that owns heap memory
//!   frees, and a region that drops one still holds the clause.
//! - "Steady state" is the author's claim: a push into a vector reserved
//!   beforehand is refused all the same, and its site answers with an `expect`
//!   that says where the capacity comes from.
//! - A call cycle is read once; a body met again while its own reading is
//!   pending counts as free, its allocations reported through the first path
//!   that reaches them.
//! - Outside the default, an instance of an unchanged generic body under a
//!   changed caller is not held.
//!
//! # Items
//!
//! - [`ALLOCATION_FREE_REGION`]: the lint.
//! - [`clause`]: the `# Cost` reader.
//! - [`Summaries`]: the per-instance reach of every body read.
//! - [`AllocationFreeRegion`]: the pass.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_hir;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use clippy_utils::is_in_test;
use quenchant_shape::shape::Maybe;
use rustc_hir::HirId;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::Level;
use rustc_lint::declare_lint;
use rustc_lint::impl_lint_pass;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty;
use rustc_middle::ty::EarlyBinder;
use rustc_middle::ty::GenericArgs;
use rustc_middle::ty::Instance;
use rustc_middle::ty::InstanceKind;
use rustc_middle::ty::ShimKind;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TypingEnv;
use rustc_span::Span;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::answer_totality::ExceptionRefusals;
use crate::answer_totality::report_defects;
use crate::answer_totality::report_refusing;
use crate::cost_shape::Ownership;
use crate::cost_shape::heap_ownership;
use crate::evidence::BodyHasher;
use crate::evidence::canonical_path;
use crate::evidence::record_name;
use crate::evidence::unit_qualifier;
use crate::semantic::AllocationWhy;
use crate::semantic::AnswersItself;
use crate::semantic::BodyScheme;
use crate::semantic::ChangedSinceBase;
use crate::semantic::CountOnly;
use crate::semantic::CrateName;
use crate::semantic::DefaultCrate;
use crate::semantic::DiagnosticText;
use crate::semantic::KnownWhy;
use crate::semantic::ProductUnit;
use crate::semantic::RecordText;
use crate::walk_summary::doc_lines;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses, in a function whose rustdoc carries a `# Cost` section with the clause `- allocation: none in steady state`, every call from which a path reaches an allocation: the allocator's entry points through any body the build can read, a body it cannot read whose signature returns or takes by `&mut` a value owning a heap allocation, and a call through a trait object, a function pointer or an unmarked trait method of a type parameter. An implementation of a trait method whose declaration carries the clause is held to it too. In a crate that `allocation_default_crates` names, it refuses each call that reaches an allocation at its origin, in a function whose `# Cost` declares none.
    ///
    /// ### Why is this bad?
    ///
    /// The function promises that once warmed up it allocates nothing; an allocation on any path breaks the promise on the path the inputs select, and pays the allocator on every item that takes it. Where the default turns round, an allocation that no line declares is one that nobody chose.
    ///
    /// ### Accepted forms
    ///
    /// A call into `core`, a call that returns `!`, a call that reaches no allocation, and a call under `#[expect(allocation_free_region, reason = "...")]` at the site. A value's destructor is not followed. Where the default turns round, a function whose `# Cost` carries `- allocation: <what>, <why>`, with one of the five whys, and a call of a function that the default holds.
    ///
    /// ### Clippy
    ///
    /// No Clippy lint follows allocation through calls.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// /// # Cost
    /// /// - allocation: none in steady state.
    /// fn record(&mut self, entry: Entry) {
    ///     self.entries.push(entry);
    /// }
    /// ```
    ///
    /// Use instead: reserve before the region is entered, and write into the reserved space.
    pub ALLOCATION_FREE_REGION,
    Deny,
    "a function marked allocation-free in steady state, or held by the default, reaches an allocation"
}

/// The heading of the section that states a function's cost.
const HEADING: &str = "# Cost";

/// The start of the line inside that section that states an allocation.
const LINE: &str = "- allocation:";

/// The clause of a region that allocates nothing in steady state.
const STEADY: &str = "none in steady state";

/// The whys that take no text after them.
const PLAIN_WHYS: [&str; 3] = ["setup", "error path", "diagnostic"];

/// The whys that name their source or their key after them.
const NAMED_WHYS: [&str; 2] = ["growth within ", "durable by "];

/// The key of the crates whose default turns round.
const CRATES_KEY: &str = "allocation_default_crates";

/// The key of the base revision's evidence records.
const BASE_KEY: &str = "allocation_base_records";

/// The key of the directory of the count reports.
const COUNT_KEY: &str = "allocation_count_directory";

/// The allocator entry points of the `alloc` crate, by path.
const ALLOCATOR_ENTRIES: [&str; 4] = [
    "alloc::alloc::alloc",
    "alloc::alloc::alloc_zeroed",
    "alloc::alloc::realloc",
    "alloc::alloc::exchange_malloc",
];

/// The help that a refusal under the default carries.
const HELP: &str = concat!(
    "move the allocation out of the function, or declare it under `# Cost` as ",
    "`- allocation: <what>, <why>`, the why one of `setup`, `growth within <the source of the ",
    "capacity>`, `error path`, `diagnostic` and `durable by <the key>`; a site that keeps it ",
    "states why with `#[expect(allocation_free_region, reason = \"...\")]`",
);

/// The refusals of an exception the site does not state properly.
const REFUSALS: ExceptionRefusals = ExceptionRefusals {
    inherited: DiagnosticText(concat!(
        "an allocation exception must be stated where the allocation stands: an ",
        "`#[expect(allocation_free_region, ..)]` on an enclosing module, implementation or crate ",
        "excepts nothing inside it",
    )),
    unreasoned: DiagnosticText(concat!(
        "an allocation exception must state why the site keeps its allocation: write ",
        "`reason = \"...\"` on its `#[expect(allocation_free_region, ..)]`",
    )),
    allowed: DiagnosticText(concat!(
        "an allocation is admitted only by its function's `- allocation:` line or by ",
        "`#[expect(allocation_free_region, reason = \"...\")]`; an `allow` does not say why the ",
        "site keeps it, nor fail when it goes",
    )),
};

/// What a function's `# Cost` section states about its allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Clause
{
    /// `- allocation: none in steady state`: a region the lint follows into
    /// each call.
    Steady,
    /// `- allocation: <what>, <why>`, with one of the five whys.
    Declared,
    /// An allocation line in neither form, with the line.
    Malformed(String),
    /// No allocation line under a `# Cost` heading.
    Silent,
}

/// Read a function's `# Cost` section.
///
/// # Specification
/// - ensures: [`clause_of`] over the rustdoc lines of `def_id`.
/// - panics: none.
/// - executable: none — the outcome is the item's rustdoc, which a predicate
///   could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures mark functions, a trait method and an
///   implementation, leave a `# Cost` section with another clause unmarked,
///   declare each why and write a malformed line.
/// - witness: `tests::ui_cost_shape`
/// - witness: `tests::ui_allocation_default`
pub fn clause(
    tcx: TyCtxt<'_>,
    def_id: DefId,
) -> Clause
{
    clause_of(&doc_lines(tcx, def_id))
}

/// Read the allocation line of the `# Cost` section of rustdoc lines.
///
/// # Specification
/// - ensures: the first line that starts with `- allocation:` after a line `#
///   Cost` and before the next heading decides. With the rest `none in steady
///   state`, a trailing period allowed, it is [`Clause::Steady`]. With a rest
///   `<what>, <why>` whose what is not empty and whose why, a trailing period
///   allowed, is one of the five forms, it is [`Clause::Declared`]. Any other
///   rest is [`Clause::Malformed`]. With no such line, it is
///   [`Clause::Silent`].
/// - panics: none.
/// - executable: none — the outcome is a scan of the lines, which a predicate
///   could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the unit tests read each why, an empty growth, an unknown
///   why, a missing what, a line under another heading and the clause of a
///   region.
/// - witness: `allocation_free_region::tests::each_why_declares`
/// - witness: `allocation_free_region::tests::a_why_outside_the_five_is_malformed`
/// - witness: `allocation_free_region::tests::only_the_cost_section_is_read`
pub fn clause_of(lines: &[String]) -> Clause
{
    let Some(start) = lines.iter().position(|line| line == HEADING)
    else {
        return Clause::Silent;
    };
    let Some(line) = lines
        .iter()
        .skip(start.saturating_add(1))
        .take_while(|line| !line.starts_with('#'))
        .find(|line| line.starts_with(LINE))
    else {
        return Clause::Silent;
    };
    let rest = line
        .get(LINE.len() ..)
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.');
    if rest == STEADY {
        return Clause::Steady;
    }
    match rest.rsplit_once(", ") {
        Some((what, why)) if !what.trim().is_empty() && why_reads(AllocationWhy(why.trim())).0 => {
            Clause::Declared
        },
        Some(_) | None => Clause::Malformed(line.clone()),
    }
}

/// Whether a why is one of the five forms.
///
/// # Specification
/// - ensures: true exactly for `setup`, `error path` and `diagnostic`, and for
///   `growth within ` or `durable by ` followed by text that is not blank.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the unit tests read each form, an empty growth and an
///   unknown why.
/// - witness: `allocation_free_region::tests::each_why_declares`
/// - witness: `allocation_free_region::tests::a_why_outside_the_five_is_malformed`
#[spec(ensures: |output| !output.0 || !why.0.is_empty())]
fn why_reads(why: AllocationWhy<'_>) -> KnownWhy
{
    KnownWhy(
        PLAIN_WHYS.contains(&why.0)
            || NAMED_WHYS.iter().any(|form| {
                why.0
                    .strip_prefix(form)
                    .is_some_and(|rest| !rest.trim().is_empty())
            }),
    )
}

/// One entry of `allocation_default_crates`.
#[derive(Clone, Debug, Eq, PartialEq)]
enum CratePattern
{
    /// A crate's whole name.
    Exact(String),
    /// The start of a crate's name, written with a `*` after it.
    Prefix(String),
}

impl CratePattern
{
    /// Whether the pattern takes a crate of name `name`.
    ///
    /// # Specification
    /// - ensures: an exact pattern takes its own name alone; a prefix takes
    ///   each name that starts with it.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the unit tests take a name whole and by prefix, and
    ///   refuse a longer name and another prefix.
    /// - witness: `allocation_free_region::tests::a_pattern_takes_its_crates`
    #[spec(ensures: |output| !output.0 || !name.0.is_empty() || matches!(self, Self::Exact(whole) if whole.is_empty()))]
    fn matches(
        &self,
        name: CrateName<'_>,
    ) -> DefaultCrate
    {
        DefaultCrate(match *self {
            Self::Exact(ref whole) => name.0 == whole,
            Self::Prefix(ref start) => name.0.starts_with(start.as_str()),
        })
    }
}

quenchant_shape::reason_enum! {
    /// A key of the configuration can be left out.
    mod setting {
        /// Why a key gives no value.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unset {
            /// The configuration does not name the key.
            NotConfigured,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A unit can take no base for its functions.
    mod base_reading {
        /// Why a unit takes no base.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unread {
            /// The configuration names no base.
            NotConfigured,
            /// The base or its record of the unit cannot be read.
            Unreadable,
            /// The base holds no record of the unit: the change does not touch
            /// its package.
            NoRecord,
            /// The base's record of the unit holds no body hash of this
            /// library's scheme: another compiler or an earlier library wrote
            /// it.
            OtherScheme,
        }
    }
}

/// The configuration of the default, as one crate reads it.
#[derive(Debug)]
struct Settings
{
    /// The crates whose default turns round.
    crates: Vec<CratePattern>,
    /// The directory of the base revision's evidence records.
    base: Maybe<PathBuf, setting::Unset>,
    /// The directory of the count reports.
    count: Maybe<PathBuf, setting::Unset>,
    /// Each entry of the configuration the lint cannot read.
    defects: Vec<String>,
}

impl Default for Settings
{
    /// No crate, no directory and no defect.
    ///
    /// # Specification
    /// trivial.
    fn default() -> Self
    {
        Self {
            crates: Vec::new(),
            base: Maybe::Absent(setting::Unset::NotConfigured),
            count: Maybe::Absent(setting::Unset::NotConfigured),
            defects: Vec::new(),
        }
    }
}

/// Read the three keys of the default.
///
/// # Specification
/// - requires: `dylint_linting::init_config` has already run, which
///   [`crate::register_lints`] guarantees by calling it first.
/// - ensures: each string entry of `allocation_default_crates` other than an
///   empty one and `*` becomes a pattern, a prefix when it ends in `*`; a
///   string of `allocation_base_records` and of `allocation_count_directory`
///   becomes its directory. Every other value becomes a defect.
/// - fails: an absent, unreadable or malformed configuration yields no crate
///   and no directory.
/// - panics: none.
/// - executable: none — the configuration read outcome is not retained; reading
///   it again is a distinct fallible operation.
///
/// # Adequacy
/// - hypothesis: L3 — the default fixture names its crates whole and by prefix;
///   the refused configuration carries a number, an empty entry and `*`, each
///   reported.
/// - witness: `tests::ui_allocation_default`
/// - witness: `tests::ui_allocation_default_refused`
fn read_settings() -> Settings
{
    let mut settings = Settings::default();
    let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
    else {
        return settings;
    };
    if let Some(value) = config.get(CRATES_KEY) {
        let Some(entries) = value.as_array()
        else {
            settings
                .defects
                .push(format!("`{CRATES_KEY}` is no list of crate names"));
            return settings;
        };
        for entry in entries {
            match entry.as_str() {
                Some(name) if !name.is_empty() && name != "*" => {
                    settings.crates.push(name.strip_suffix('*').map_or_else(
                        || CratePattern::Exact(name.to_owned()),
                        |start| CratePattern::Prefix(start.to_owned()),
                    ));
                },
                Some(_) | None => settings.defects.push(format!(
                    "`{CRATES_KEY}` entry `{entry}` is no crate name, nor the start of one followed by \
                     `*`"
                )),
            }
        }
    }
    for (key, slot) in [
        (BASE_KEY, &mut settings.base),
        (COUNT_KEY, &mut settings.count),
    ] {
        let Some(value) = config.get(key)
        else {
            continue;
        };
        match value.as_str() {
            Some(directory) if !directory.is_empty() => {
                *slot = Maybe::Present(PathBuf::from(directory));
            },
            Some(_) | None => settings.defects.push(format!(
                "`{key}` is `{value}`, and not the path of a directory"
            )),
        }
    }
    settings
}

/// The body hashes of the functions of one unit's base record, when the record
/// holds hashes of this library's scheme.
///
/// # Specification
/// - ensures: for a `text` with no line, an empty map: the base holds the unit
///   with no function. Otherwise, for each line that is a JSON `item` record of
///   kind `fn` or `assoc-fn` with a string `body`, its `path` maps to its
///   `body`, when some such body starts with `scheme` and a hyphen; every other
///   line adds nothing.
/// - provides: `base_reading::Unread::OtherScheme` for a `text` with a line and
///   no body of `scheme`.
/// - panics: none.
/// - executable: none — the outcome is a parse of the text, which a predicate
///   could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the unit test reads a function, a method, a function with
///   no body hash, a type and a line that is no JSON; it reads an empty text,
///   and a text whose bodies carry another scheme.
/// - witness: `allocation_free_region::tests::the_base_maps_each_function_to_its_body`
/// - witness: `allocation_free_region::tests::a_base_of_another_scheme_is_no_base`
fn base_bodies_of(
    text: RecordText<'_>,
    scheme: BodyScheme<'_>,
) -> Maybe<HashMap<String, String>, base_reading::Unread>
{
    if text.0.lines().all(|line| line.trim().is_empty()) {
        return Maybe::Present(HashMap::new());
    }
    let bodies: HashMap<String, String> = text
        .0
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|record| {
            record.get("record").and_then(serde_json::Value::as_str) == Some("item")
                && matches!(
                    record.get("kind").and_then(serde_json::Value::as_str),
                    Some("fn" | "assoc-fn")
                )
        })
        .filter_map(|record| {
            let path = record.get("path")?.as_str()?.to_owned();
            let body = record.get("body")?.as_str()?.to_owned();
            Some((path, body))
        })
        .collect();
    if bodies.values().any(|body| {
        body.strip_prefix(scheme.0)
            .is_some_and(|rest| rest.starts_with('-'))
    }) {
        Maybe::Present(bodies)
    }
    else {
        Maybe::Absent(base_reading::Unread::OtherScheme)
    }
}

/// Whether the unit compiles a product: a library or a binary that is no
/// test, no benchmark and no example.
///
/// # Specification
/// - ensures: false for a test harness, for a unit for which Cargo sets
///   `CARGO_TARGET_TMPDIR` (an integration test or a benchmark), and for a unit
///   whose root file lies under an `examples` or a `benches` directory; true
///   otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's session and environment,
///   which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the default fixtures compile as a binary and are read;
///   the test-target fixture compiles under `--test` and reads only its
///   expected site.
/// - witness: `tests::ui_allocation_default`
/// - witness: `tests::ui_allocation_default_test_target`
fn product_unit(tcx: TyCtxt<'_>) -> ProductUnit
{
    if tcx.sess.is_test_crate() || std::env::var_os("CARGO_TARGET_TMPDIR").is_some() {
        return ProductUnit(false);
    }
    ProductUnit(
        !tcx.sess
            .local_crate_source_file()
            .and_then(|file| file.local_path().map(Path::to_path_buf))
            .is_some_and(|root| {
                root.components()
                    .any(|part| matches!(part.as_os_str().to_str(), Some("examples" | "benches")))
            }),
    )
}

/// What reaches an allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause
{
    /// A path reaches the allocator's entry point.
    Allocator,
    /// A body the build cannot read may allocate by its signature.
    Signature,
    /// A call through a trait object or a function pointer, or of a trait
    /// method that resolves to no body.
    Dynamic,
    /// A trait method of a type parameter that the generic root cannot
    /// resolve.
    Parameter,
}

/// The first allocation a body reaches, and where.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Witness
{
    /// What allocates.
    pub cause: Cause,
    /// The call that allocates, in the body that makes it.
    pub span: Span,
    /// The function whose body makes that call.
    pub owner: DefId,
}

/// Whether a body reaches an allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reach
{
    /// Some path reaches the witnessed allocation.
    Allocates(Witness),
    /// No path does.
    Free,
}

/// What one call reaches, once its callee is resolved.
#[derive(Clone, Copy, Debug)]
enum Call<'tcx>
{
    /// The call cannot allocate, or ends the steady state.
    Free,
    /// The call allocates directly, or may.
    Allocates(Cause),
    /// The call's body must be summarized.
    Body(Instance<'tcx>),
}

/// How a reading treats the calls it meets.
#[derive(Clone, Copy, Debug)]
enum Mode<'crates>
{
    /// A marked region: every call is followed.
    Every,
    /// The default: a call of a function that the default holds is free, and
    /// so is a type parameter's trait method.
    Origin(&'crates [CratePattern]),
}

/// Whether a callee answers for its own allocations under the default.
///
/// # Specification
/// - ensures: true exactly when `def_id` is a function or method that no macro
///   of another crate expands, of this crate or of a crate that one of `crates`
///   takes.
/// - panics: none.
/// - executable: none — the outcome is the compiler's definition tables, which
///   a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the default fixture calls a local helper that declares
///   its allocation, a closure that allocates and a derived `Clone` of a
///   vector; the first is free and the other two are refused.
/// - witness: `tests::ui_allocation_default`
fn answers(
    tcx: TyCtxt<'_>,
    crates: &[CratePattern],
    def_id: DefId,
) -> AnswersItself
{
    AnswersItself(
        matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn)
            && !tcx
                .def_span(def_id)
                .in_external_macro(tcx.sess.source_map())
            && (def_id.is_local()
                || crates.iter().any(|pattern| {
                    pattern
                        .matches(CrateName(tcx.crate_name(def_id.krate).as_str()))
                        .0
                })),
    )
}

/// One call as a reading in `mode` takes it.
///
/// # Specification
/// - ensures: in [`Mode::Every`] the call stays as it is. In [`Mode::Origin`],
///   a type parameter's trait method is free, and so is a body whose function
///   [`answers`]; every other call stays as it is.
/// - panics: none.
/// - executable: none — the outcome is [`answers`] over the call, which a
///   predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the default fixture's generic root calls a trait method
///   of its parameter and is accepted; the marked fixture's is refused.
/// - witness: `tests::ui_allocation_default`
/// - witness: `tests::ui_cost_shape`
fn bounded<'tcx>(
    tcx: TyCtxt<'tcx>,
    mode: Mode<'_>,
    call: Call<'tcx>,
) -> Call<'tcx>
{
    let Mode::Origin(crates) = mode
    else {
        return call;
    };
    match call {
        Call::Allocates(Cause::Parameter) => Call::Free,
        Call::Body(instance) if answers(tcx, crates, instance.def_id()).0 => Call::Free,
        Call::Free | Call::Allocates(_) | Call::Body(_) => call,
    }
}

/// The summaries computed so far in one compilation, by instance.
#[derive(Default)]
#[repr(transparent)]
pub struct Summaries<'tcx>(HashMap<Instance<'tcx>, Reach>);

/// The outcome of reading one body.
enum Reading<'tcx>
{
    /// Every body the reading depends on was summarized.
    Ready(Reach),
    /// These bodies must be summarized first.
    Needs(Vec<Instance<'tcx>>),
}

impl<'tcx> Summaries<'tcx>
{
    /// The reach of `instance`, computing it and every body it reaches.
    ///
    /// # Specification
    /// - requires: every call of this map uses one mode, since the reach of a
    ///   body depends on it.
    /// - ensures: the reach of `instance` is recorded afterwards, with that of
    ///   every body it reaches; a body reached again while its own reading is
    ///   pending counts as free, so a call cycle ends.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse an allocation two calls deep and
    ///   through a closure, and accept a recursive pair that allocates nothing.
    /// - witness: `tests::ui_cost_shape`
    /// - witness: `tests::ui_allocation_default`
    #[spec(ensures: |_| self.0.contains_key(&instance))]
    fn of(
        &mut self,
        tcx: TyCtxt<'tcx>,
        typing_env: TypingEnv<'tcx>,
        instance: Instance<'tcx>,
        mode: Mode<'_>,
    ) -> Reach
    {
        let mut pending = vec![instance];
        let mut reading = HashSet::new();
        while let Some(&current) = pending.last() {
            if self.0.contains_key(&current) {
                pending.pop();
                reading.remove(&current);
                continue;
            }
            match read_body(tcx, typing_env, &self.0, &reading, current, mode) {
                Reading::Ready(reach) => {
                    self.0.insert(current, reach);
                    pending.pop();
                    reading.remove(&current);
                },
                Reading::Needs(callees) => {
                    reading.insert(current);
                    pending.extend(callees);
                },
            }
        }
        self.0.get(&instance).copied().unwrap_or(Reach::Free)
    }
}

/// The calls of one body, each with what it reaches and its site.
///
/// # Specification
/// - ensures: one entry per call terminator of `instance`'s MIR, in block
///   order, with its span, its lint root and [`classify_call`]'s answer.
/// - panics: none.
/// - executable: none — the outcome is a filter over the body's terminators.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call allocating and free functions, closures
///   and trait methods from marked bodies.
/// - witness: `tests::ui_cost_shape`
fn calls<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    instance: Instance<'tcx>,
) -> Vec<(Span, Maybe<HirId, root_reading::NoRoot>, Call<'tcx>)>
{
    let body = tcx.instance_mir(instance.def);
    let mut found = Vec::new();
    for block in body.basic_blocks.iter() {
        let Some(terminator) = block.terminator.as_ref()
        else {
            continue;
        };
        let TerminatorKind::Call {
            ref func, fn_span, ..
        } = terminator.kind
        else {
            continue;
        };
        let callee = instance.instantiate_mir_and_normalize_erasing_regions(
            tcx,
            typing_env,
            EarlyBinder::bind(tcx, func.ty(&body.local_decls, tcx)),
        );
        let root = terminator
            .source_info
            .scope
            .lint_root(&body.source_scopes)
            .map_or(Maybe::Absent(root_reading::NoRoot::Foreign), Maybe::Present);
        found.push((fn_span, root, classify_call(tcx, typing_env, callee)));
    }
    found
}

/// Read one body against the summaries computed so far.
///
/// # Specification
/// - ensures: [`Reading::Needs`] lists the callees whose summaries are missing
///   and not themselves being read; otherwise [`Reading::Ready`] holds the
///   first call, in block order, that allocates or whose callee's summary
///   allocates, each call taken as [`bounded`] gives it in `mode`, or `Free`.
/// - panics: none.
/// - executable: none — the outcome is a scan over the body's calls against the
///   summaries; a predicate could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse an allocation two calls deep.
/// - witness: `tests::ui_cost_shape`
/// - witness: `tests::ui_allocation_default`
fn read_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    done: &HashMap<Instance<'tcx>, Reach>,
    reading: &HashSet<Instance<'tcx>>,
    instance: Instance<'tcx>,
    mode: Mode<'_>,
) -> Reading<'tcx>
{
    let mut needs = Vec::new();
    let mut first = Reach::Free;
    for (span, _, call) in calls(tcx, typing_env, instance) {
        let reach = match bounded(tcx, mode, call) {
            Call::Free => Reach::Free,
            Call::Allocates(cause) => Reach::Allocates(Witness {
                cause,
                span,
                owner: instance.def_id(),
            }),
            Call::Body(callee) => match done.get(&callee) {
                Some(&reach) => reach,
                None if reading.contains(&callee) => Reach::Free,
                None => {
                    needs.push(callee);
                    Reach::Free
                },
            },
        };
        if first == Reach::Free {
            first = reach;
        }
    }
    if needs.is_empty() {
        Reading::Ready(first)
    }
    else {
        Reading::Needs(needs)
    }
}

quenchant_shape::reason_enum! {
    /// A call's source scope can lack a lint root.
    mod root_reading {
        /// Why a call carries no lint root.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NoRoot {
            /// The body was inlined from another crate's scope.
            Foreign,
        }
    }
}

/// Whether a callee's crate is one whose functions never allocate.
///
/// # Specification
/// - ensures: `Never` exactly when `def_id` belongs to `core`; `Possible`
///   otherwise.
/// - panics: none.
/// - executable: none — the outcome is the crate store's name for the callee's
///   crate.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call `core` arithmetic and slice reads from
///   marked bodies.
/// - witness: `tests::ui_cost_shape`
fn in_core(
    tcx: TyCtxt<'_>,
    def_id: DefId,
) -> Allocation
{
    if tcx.crate_name(def_id.krate).as_str() == "core" {
        Allocation::Never
    }
    else {
        Allocation::Possible
    }
}

/// What a callee's crate allows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Allocation
{
    /// The callee cannot allocate.
    Never,
    /// The callee may.
    Possible,
}

/// Classify one call by its callee type.
///
/// # Specification
/// - ensures: `Free` for a callee returning `!`, a callee resolving to a `core`
///   item instantiated with no types (a generic one is read, since its type
///   arguments may bring closures and implementations that allocate), an
///   intrinsic or a drop shim; `Allocates(Allocator)` for the allocator's entry
///   points; `Allocates(Dynamic)` for a call through a function pointer or a
///   trait object whose declaration is unmarked; `Allocates(Parameter)` for a
///   trait method that resolves to no instance and whose declaration is
///   unmarked; `Free` for an unresolved trait method whose declaration is
///   marked; `Body` for a resolved instance whose MIR is available;
///   `Allocates(Signature)` or `Free` by [`signature_allocates`] otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a push, a call through `dyn`, an
///   unmarked trait method of a type parameter, and accept a marked trait
///   method of a type parameter, a panic path and `core` calls.
/// - witness: `tests::ui_cost_shape`
fn classify_call<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    callee: Ty<'tcx>,
) -> Call<'tcx>
{
    let ty::FnDef(def_id, args) = *callee.kind()
    else {
        return Call::Allocates(Cause::Dynamic);
    };
    let output = tcx.fn_sig(def_id).skip_binder().output().skip_binder();
    if output.is_never() {
        return Call::Free;
    }
    let path = tcx.def_path_str(def_id);
    if ALLOCATOR_ENTRIES.contains(&path.as_str())
        || matches!(
            tcx.item_name(def_id).as_str(),
            "__rust_alloc" | "__rust_alloc_zeroed" | "__rust_realloc" | "box_new"
        )
    {
        return Call::Allocates(Cause::Allocator);
    }
    let Some(args) = args.no_bound_vars()
    else {
        return trait_call(tcx, def_id, Cause::Parameter);
    };
    let Ok(Some(instance)) = Instance::try_resolve(tcx, typing_env, def_id, args)
    else {
        return trait_call(tcx, def_id, Cause::Parameter);
    };
    match instance.def {
        InstanceKind::Virtual(..) => trait_call(tcx, def_id, Cause::Dynamic),
        InstanceKind::Shim(ShimKind::DropGlue(..) | ShimKind::AsyncDropGlue(..))
        | InstanceKind::Intrinsic(_)
        | InstanceKind::LlvmIntrinsic(_) => Call::Free,
        InstanceKind::Item(item)
            if in_core(tcx, item) == Allocation::Never
                && instance.args.types().next().is_none() =>
        {
            Call::Free
        },
        InstanceKind::Item(item) if tcx.is_mir_available(item) => Call::Body(instance),
        InstanceKind::Item(item) => signature_allocates(tcx, typing_env, item),
        InstanceKind::Shim(
            ShimKind::VTable(_)
            | ShimKind::Reify(..)
            | ShimKind::FnPtr(..)
            | ShimKind::ClosureOnce { .. }
            | ShimKind::ConstructCoroutineInClosure { .. }
            | ShimKind::ThreadLocal(_)
            | ShimKind::FutureDropPoll(..)
            | ShimKind::AsyncDropGlueCtor(..)
            | ShimKind::Clone(..)
            | ShimKind::FnPtrAsPtr(..)
            | ShimKind::FnPtrFromPtr(..),
        ) => Call::Body(instance),
    }
}

/// Classify a call that resolves to no concrete body.
///
/// # Specification
/// - ensures: `Free` when the callee's trait method declaration is marked;
///   `Allocates(cause)` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the declaration's rustdoc, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a marked and an unmarked trait method
///   of a type parameter.
/// - witness: `tests::ui_cost_shape`
fn trait_call(
    tcx: TyCtxt<'_>,
    def_id: DefId,
    cause: Cause,
) -> Call<'_>
{
    if clause(tcx, def_id) == Clause::Steady {
        Call::Free
    }
    else {
        Call::Allocates(cause)
    }
}

/// Classify a callee whose body the build cannot read, by its signature.
///
/// # Specification
/// - ensures: `Allocates(Signature)` exactly when the callee returns a value
///   [`heap_ownership`] reads as owning, refcounted or unknown, or takes a
///   `&mut` to one; `Free` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's signature and layout
///   reading, which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a foreign function returning a `Vec`
///   and one returning a count.
/// - witness: `tests::ui_cost_shape`
fn signature_allocates<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    item: DefId,
) -> Call<'tcx>
{
    let signature = tcx.instantiate_bound_regions_with_erased(
        tcx.fn_sig(item).instantiate_identity().skip_norm_wip(),
    );
    let heavy = |held: Ty<'tcx>| heap_ownership(tcx, typing_env, held) != Ownership::Plain;
    let returns = heavy(signature.output());
    let writes = signature.inputs().iter().any(|input| {
        if let ty::Ref(_, pointee, ty::Mutability::Mut) = *input.kind() {
            heavy(pointee)
        }
        else {
            false
        }
    });
    if returns || writes {
        Call::Allocates(Cause::Signature)
    }
    else {
        Call::Free
    }
}

/// What a reason names, for a diagnostic.
///
/// # Specification
/// - ensures: one phrase for each cause.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L2 — the fixtures' refusals print each phrase.
/// - witness: `tests::ui_cost_shape`
/// - witness: `tests::ui_allocation_default`
#[spec(ensures: |output| !output.0.is_empty())]
fn reason(cause: Cause) -> DiagnosticText<'static>
{
    DiagnosticText(match cause {
        Cause::Allocator => "a call into the allocator",
        Cause::Signature => {
            "a call whose body is unavailable and whose signature hands back or writes a value that \
             owns heap memory"
        },
        Cause::Dynamic | Cause::Parameter => {
            "a call that can reach any implementation, none of them held to the clause"
        },
    })
}

/// The count of one unit under the default, for its report.
#[derive(Clone, Copy, Debug, Default)]
struct Tally
{
    /// The functions the default holds.
    held: usize,
    /// The functions whose `# Cost` declares an allocation.
    declared: usize,
    /// The held functions that reach the allocator.
    allocator: usize,
    /// The held functions that reach a signature that owns heap memory.
    signature: usize,
    /// The held functions that call through `dyn` or a function pointer.
    dynamic: usize,
}

impl Tally
{
    /// Count one refused function by the cause of its first refusal.
    ///
    /// # Specification
    /// - ensures: the counter of `cause` grows by one; a parameter's cause
    ///   counts as dynamic.
    /// - panics: none.
    /// - executable: none — the counter that grows is chosen by `cause`, and a
    ///   predicate would restate that choice.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the count fixture's report pins each counter.
    /// - witness: `tests::ui_allocation_count`
    fn refuse(
        &mut self,
        cause: Cause,
    )
    {
        let counter = match cause {
            Cause::Allocator => &mut self.allocator,
            Cause::Signature => &mut self.signature,
            Cause::Dynamic | Cause::Parameter => &mut self.dynamic,
        };
        *counter = counter.saturating_add(1);
    }
}

/// The marked regions and the default whose calls this lint reads.
///
/// A consumer turns the default round crate by crate. The key
/// `allocation_default_crates` under `[quenchant-dylints]` names each crate,
/// whole or as a prefix that ends in `*`. In such a crate the lint holds each
/// function and method that is no test and that no macro of another crate
/// expands. A function that allocates declares it with one line under its
/// `# Cost` heading: `- allocation: <what>, <why>`. The why is one of five
/// forms: `setup`, `growth within <the source of the capacity>`,
/// `error path`, `diagnostic` and `durable by <the key>`. Another why, and an
/// allocation line in neither form, is refused.
///
/// The lint refuses an allocation at its origin. A call of a function that
/// the default holds, in this crate or in a crate the key names, is free, and
/// that function answers for itself. A closure, a body that a macro of
/// another crate expands, and a body of any other crate are read in turn. At
/// a generic root, a trait method of a type parameter is free: the lint reads
/// the implementation where a caller's instance resolves the parameter.
///
/// A site that keeps its allocation states why with
/// `#[expect(allocation_free_region, reason = "…")]` at the call. An
/// expectation on an enclosing module, implementation or crate, one without
/// its reason and an `allow` written in the source are refused. A test
/// target reads only the sites that state an expectation, so the expectation
/// of a library holds in its test build too. An integration test, a
/// benchmark and an example are not read.
///
/// The key `allocation_base_records` names the evidence records of a base
/// revision. Outside the default, the lint then holds each function whose
/// [`crate::evidence::BodyHasher::body`] differs from the base's record, or
/// that the base's record of the unit lacks. A unit whose base holds no record
/// of it, or a record with no hash of [`crate::evidence::body_scheme`], takes
/// no base and holds nothing: its package is untouched, or another compiler or
/// an earlier library wrote the record. An empty record is a unit new at the
/// base, and each of its functions is held. The key
/// `allocation_count_directory` names a directory. There the lint writes one
/// report for each library or binary unit: the functions it holds, refuses and
/// finds declared. It refuses nothing then.
pub struct AllocationFreeRegion<'tcx>
{
    /// The summaries of the marked regions, which follow every call.
    summaries: Summaries<'tcx>,
    /// The summaries of the default, which stop at a function it holds.
    origin: Summaries<'tcx>,
    /// The configuration of the default.
    settings: Settings,
    /// The body hash of each function of the base's record of this unit.
    base: Maybe<HashMap<String, String>, base_reading::Unread>,
}

impl_lint_pass!(AllocationFreeRegion<'_> => [ALLOCATION_FREE_REGION]);

impl AllocationFreeRegion<'_>
{
    /// The pass, with the configuration of the default read.
    ///
    /// # Specification
    /// - requires: as [`read_settings`].
    /// - ensures: empty summaries, the settings [`read_settings`] reads, and no
    ///   base record yet; [`LateLintPass::check_crate`] reads it.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — as [`read_settings`].
    /// - witness: `tests::ui_allocation_default`
    pub fn configured() -> Self
    {
        Self {
            summaries: Summaries::default(),
            origin: Summaries::default(),
            settings: read_settings(),
            base: Maybe::Absent(base_reading::Unread::NotConfigured),
        }
    }
}

/// The clause a function is held to.
///
/// # Specification
/// - ensures: the function's own [`clause`] when it states one; otherwise
///   `Steady` when it implements a trait method whose declaration carries the
///   clause of a region, and `Silent` else.
/// - panics: none.
/// - executable: none — the outcome is the item's and its trait declaration's
///   rustdoc, which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse an implementation of a marked trait
///   method that pushes, and accept an unmarked function that does.
/// - witness: `tests::ui_cost_shape`
fn held(
    tcx: TyCtxt<'_>,
    def_id: LocalDefId,
) -> Clause
{
    let own = clause(tcx, def_id.to_def_id());
    if own != Clause::Silent {
        return own;
    }
    match tcx.trait_item_of(def_id.to_def_id()) {
        Some(declaration) if clause(tcx, declaration) == Clause::Steady => Clause::Steady,
        Some(_) | None => Clause::Silent,
    }
}

/// The span a refusal of a call is reported at.
///
/// rustc drops a lint whose span lies in a macro of another crate. Without
/// this span, a `vec!` or a `format!` in a held body is not reported.
///
/// # Specification
/// - ensures: `span` when it lies outside every macro of another crate;
///   otherwise the call site of the outermost expansion that holds it.
/// - panics: none.
/// - executable: none — the outcome is the source map's reading of the
///   expansion, which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the marked fixture writes `format!` in a region, and the
///   default fixtures write `vec!` and `format!` in held bodies; each is
///   refused at the macro's call.
/// - witness: `tests::ui_cost_shape`
/// - witness: `tests::ui_allocation_default`
fn reported_at(
    tcx: TyCtxt<'_>,
    span: Span,
) -> Span
{
    if span.in_external_macro(tcx.sess.source_map()) {
        span.source_callsite()
    }
    else {
        span
    }
}

impl<'tcx> AllocationFreeRegion<'tcx>
{
    /// Refuse every call of a marked function that reaches an allocation.
    ///
    /// # Specification
    /// - ensures: one refusal per call in the MIR of `owner`, in block order,
    ///   whose callee is not free when every call is followed, reported at
    ///   [`reported_at`] the call under the call's own lint level.
    /// - panics: none.
    /// - executable: none — the outcome is emitted diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse direct, nested, dynamic and
    ///   signature-read allocations and a marked trait method's implementation,
    ///   and accept marked bodies reaching only `core`, panics and marked trait
    ///   methods, and a site under `expect`.
    /// - witness: `tests::ui_cost_shape`
    fn refuse_marked(
        &mut self,
        cx: &LateContext<'tcx>,
        owner: LocalDefId,
    )
    {
        let tcx = cx.tcx;
        let typing_env = TypingEnv::post_analysis(tcx, owner);
        let root = Instance::new_raw(
            owner.to_def_id(),
            GenericArgs::identity_for_item(tcx, owner),
        );
        for (span, lint_root, call) in calls(tcx, typing_env, root) {
            let reach = match call {
                Call::Free => Reach::Free,
                Call::Allocates(cause) => Reach::Allocates(Witness {
                    cause,
                    span,
                    owner: owner.to_def_id(),
                }),
                Call::Body(callee) => self.summaries.of(tcx, typing_env, callee, Mode::Every),
            };
            let Reach::Allocates(witness) = reach
            else {
                continue;
            };
            let hir_id = match lint_root {
                Maybe::Present(hir_id) => hir_id,
                Maybe::Absent(_) => tcx.local_def_id_to_hir_id(owner),
            };
            span_lint_hir_and_then(
                cx,
                ALLOCATION_FREE_REGION,
                hir_id,
                reported_at(tcx, span),
                "this call can allocate inside a function marked allocation-free in steady state",
                |diag| {
                    let reason = reason(witness.cause).0;
                    if witness.span == span {
                        diag.note(format!("this is {reason}"));
                    }
                    else if witness.owner.is_local() {
                        diag.span_note(witness.span, format!("the path reaches {reason}"));
                    }
                    else {
                        diag.note(format!(
                            "the path reaches {reason}, in `{}`",
                            tcx.def_path_str(witness.owner)
                        ));
                    }
                    diag.help(
                        "reserve or build what the region needs before it is entered, or mark the \
                         callee with the same clause",
                    );
                },
            );
        }
    }

    /// Whether the base's record changes or lacks a function.
    ///
    /// # Specification
    /// - ensures: false with no base; otherwise true exactly when the base's
    ///   record has no body hash for the function's path, or one other than its
    ///   [`BodyHasher::body`] now.
    /// - panics: none.
    /// - executable: none — the outcome is the hash and the base's record,
    ///   which a predicate could only read again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the base fixture keeps one function, changes one and
    ///   adds one; the last two alone are held.
    /// - witness: `tests::ui_allocation_base`
    fn changed(
        &self,
        bodies: &mut BodyHasher<'_>,
        tcx: TyCtxt<'_>,
        qualifier: &Maybe<String, crate::evidence::unit_paths::Unqualified>,
        owner: LocalDefId,
    ) -> ChangedSinceBase
    {
        let Maybe::Present(ref recorded) = self.base
        else {
            return ChangedSinceBase(false);
        };
        let Maybe::Present(hash) = bodies.body(owner)
        else {
            return ChangedSinceBase(false);
        };
        ChangedSinceBase(
            recorded.get(&canonical_path(tcx, qualifier, owner.to_def_id())) != Some(&hash),
        )
    }

    /// Refuse, or count, each call of a held function that reaches an
    /// allocation at its origin.
    ///
    /// # Specification
    /// - ensures: for each call of `owner` that reaches an allocation when a
    ///   function the default holds answers for itself, one refusal at
    ///   [`reported_at`] the call with [`report_refusing`], which also refuses
    ///   an exception the site does not state properly. In a test target, a
    ///   site whose level is no expectation is not reported. When `count`
    ///   holds, nothing is reported. Returns the cause of the first such call,
    ///   or `Absent` when no call reaches one.
    /// - panics: none.
    /// - executable: none — the outcome is emitted diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the default fixture refuses a push, a `format!`, a
    ///   call through `dyn`, a closure that allocates and a derived `Clone` of
    ///   a vector, and accepts a reasoned site; the refused exceptions are
    ///   reported at the crate root.
    /// - witness: `tests::ui_allocation_default`
    /// - witness: `tests::ui_allocation_default_test_target`
    fn refuse_origin(
        &mut self,
        cx: &LateContext<'tcx>,
        owner: LocalDefId,
        count: CountOnly,
    ) -> Maybe<Cause, origin_reading::NoAllocation>
    {
        let tcx = cx.tcx;
        let typing_env = TypingEnv::post_analysis(tcx, owner);
        let root = Instance::new_raw(
            owner.to_def_id(),
            GenericArgs::identity_for_item(tcx, owner),
        );
        let mode = Mode::Origin(&self.settings.crates);
        let mut first = Maybe::Absent(origin_reading::NoAllocation::Free);
        for (span, lint_root, call) in calls(tcx, typing_env, root) {
            let reach = match bounded(tcx, mode, call) {
                Call::Free => Reach::Free,
                Call::Allocates(cause) => Reach::Allocates(Witness {
                    cause,
                    span,
                    owner: owner.to_def_id(),
                }),
                Call::Body(callee) => self.origin.of(tcx, typing_env, callee, mode),
            };
            let Reach::Allocates(witness) = reach
            else {
                continue;
            };
            if matches!(first, Maybe::Absent(_)) {
                first = Maybe::Present(witness.cause);
            }
            if count.0 {
                continue;
            }
            let hir_id = match lint_root {
                Maybe::Present(hir_id) => hir_id,
                Maybe::Absent(_) => tcx.local_def_id_to_hir_id(owner),
            };
            if tcx.sess.is_test_crate()
                && tcx
                    .lint_level_spec_at_node(ALLOCATION_FREE_REGION, hir_id)
                    .level()
                    != Level::Expect
            {
                continue;
            }
            let place = if witness.span == span || witness.owner.is_local() {
                String::new()
            }
            else {
                format!(", in `{}`", tcx.def_path_str(witness.owner))
            };
            report_refusing(
                cx,
                ALLOCATION_FREE_REGION,
                hir_id,
                reported_at(tcx, span),
                format!(
                    "this call reaches {}{place}, and its function's `# Cost` declares no allocation",
                    reason(witness.cause).0
                ),
                DiagnosticText(HELP),
                &REFUSALS,
            );
        }
        first
    }
}

quenchant_shape::reason_enum! {
    /// A held function can reach no allocation at its origin.
    mod origin_reading {
        /// Why a held function is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NoAllocation {
            /// No call of the function reaches an allocation.
            Free,
        }
    }
}

impl<'tcx> LateLintPass<'tcx> for AllocationFreeRegion<'tcx>
{
    /// Read the base's record of this unit, and report the configuration's
    /// defects.
    ///
    /// # Specification
    /// - ensures: with a base directory that exists, the outcome of
    ///   [`base_bodies_of`] over the file of this unit's record name in it,
    ///   under [`crate::evidence::body_scheme`]. A base with no such file gives
    ///   `NoRecord`, and a file that cannot be read gives `Unreadable` and a
    ///   defect. A base directory that does not exist is a defect, and then no
    ///   function is held for a change. Each defect is reported at the crate
    ///   root.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the base fixture reads its unit's file, an empty
    ///   file, no file and a file of another scheme; the refused configuration
    ///   names a base directory that does not exist.
    /// - witness: `tests::ui_allocation_base`
    /// - witness: `tests::ui_allocation_base_new`
    /// - witness: `tests::ui_allocation_base_unread`
    /// - witness: `tests::ui_allocation_default_refused`
    fn check_crate(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        if let Maybe::Present(ref directory) = self.settings.base {
            if directory.is_dir() {
                let scheme = crate::evidence::body_scheme();
                self.base = match std::fs::read_to_string(directory.join(record_name(cx.tcx))) {
                    Ok(text) => base_bodies_of(RecordText(&text), BodyScheme(&scheme)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        Maybe::Absent(base_reading::Unread::NoRecord)
                    },
                    Err(error) => {
                        self.settings.defects.push(format!(
                            "`{BASE_KEY}` holds a record of this unit that cannot be read: {error}"
                        ));
                        Maybe::Absent(base_reading::Unread::Unreadable)
                    },
                };
            }
            else {
                self.base = Maybe::Absent(base_reading::Unread::Unreadable);
                self.settings.defects.push(format!(
                    "`{BASE_KEY}` names `{}`, which is no directory",
                    directory.display()
                ));
            }
        }
        report_defects(cx, ALLOCATION_FREE_REGION, &self.settings.defects);
    }

    /// Refuse each allocation that a marked region or the default holds, and
    /// write the count when the configuration asks for it.
    ///
    /// # Specification
    /// - ensures: a function [`held`] to a region is refused as
    ///   [`AllocationFreeRegion::refuse_marked`] says. A malformed allocation
    ///   line is refused at its function. A silent function that is no test,
    ///   that no macro of another crate expands, and that the default holds,
    ///   that the count holds or that the base's record changes or lacks, is
    ///   refused as [`AllocationFreeRegion::refuse_origin`] says, in a product
    ///   unit or a test target. With a count directory, a product unit writes
    ///   one report line there under its record name and is refused nothing
    ///   under the default.
    /// - panics: none.
    /// - executable: none — the outcome is emitted diagnostics and a report.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the marked, default, base and count fixtures pin
    ///   their refusals and their report.
    /// - witness: `tests::ui_cost_shape`
    /// - witness: `tests::ui_allocation_default`
    /// - witness: `tests::ui_allocation_base`
    /// - witness: `tests::ui_allocation_count`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let tcx = cx.tcx;
        let crate_name = tcx.crate_name(LOCAL_CRATE);
        let default = self
            .settings
            .crates
            .iter()
            .any(|pattern| pattern.matches(CrateName(crate_name.as_str())).0);
        let product = product_unit(tcx).0;
        let counting = product && matches!(self.settings.count, Maybe::Present(_));
        let read = product || tcx.sess.is_test_crate();
        let qualifier = unit_qualifier();
        let mut bodies = BodyHasher::new(tcx);
        let mut tally = Tally::default();
        for owner in tcx.hir_body_owners() {
            if !matches!(tcx.def_kind(owner), DefKind::Fn | DefKind::AssocFn) {
                continue;
            }
            let hir_id = tcx.local_def_id_to_hir_id(owner);
            match held(tcx, owner) {
                Clause::Steady => {
                    self.refuse_marked(cx, owner);
                    continue;
                },
                Clause::Malformed(line) => {
                    span_lint_hir(
                        cx,
                        ALLOCATION_FREE_REGION,
                        hir_id,
                        tcx.def_span(owner),
                        format!(
                            "the `# Cost` line `{line}` states an allocation in neither form: write \
                             `- allocation: none in steady state`, or `- allocation: <what>, <why>` \
                             with the why one of `setup`, `growth within <the source of the \
                             capacity>`, `error path`, `diagnostic` and `durable by <the key>`"
                        ),
                    );
                    continue;
                },
                Clause::Declared => {
                    tally.declared = tally.declared.saturating_add(1);
                    continue;
                },
                Clause::Silent => {},
            }
            if !read
                || is_in_test(tcx, hir_id)
                || tcx.def_span(owner).in_external_macro(tcx.sess.source_map())
            {
                continue;
            }
            if !(default || counting || self.changed(&mut bodies, tcx, &qualifier, owner).0) {
                continue;
            }
            tally.held = tally.held.saturating_add(1);
            if let Maybe::Present(cause) = self.refuse_origin(cx, owner, CountOnly(counting)) {
                tally.refuse(cause);
            }
        }
        if counting && let Maybe::Present(ref directory) = self.settings.count {
            let line = serde_json::json!({
                "unit": record_name(tcx),
                "crate": crate_name.as_str(),
                "held": tally.held,
                "refused": tally.allocator.saturating_add(tally.signature).saturating_add(tally.dynamic),
                "declared": tally.declared,
                "allocator": tally.allocator,
                "signature": tally.signature,
                "dynamic": tally.dynamic,
            });
            let written = std::fs::create_dir_all(directory).and_then(|()| {
                std::fs::write(directory.join(record_name(tcx)), format!("{line}\n"))
            });
            if let Err(error) = written {
                report_defects(cx, ALLOCATION_FREE_REGION, &[format!(
                    "the count of this unit is not written to `{}`: {error}",
                    directory.display()
                )]);
            }
        }
    }
}

#[cfg(test)]
mod tests
{
    use anodized::spec;
    use quenchant_shape::shape::Maybe;

    use super::Clause;
    use super::CratePattern;
    use super::base_bodies_of;
    use super::base_reading;
    use super::clause_of;
    use crate::semantic::BodyScheme;
    use crate::semantic::CrateName;
    use crate::semantic::RecordText;
    use crate::semantic::RustdocText;

    /// The rustdoc lines of a `# Cost` section with one allocation line.
    ///
    /// # Specification
    /// - ensures: a lead line, the `# Cost` heading, `line`, and an `#
    ///   Adequacy` section with an allocation line that no reader takes.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — each clause test reads its lines through it.
    /// - witness: `allocation_free_region::tests::each_why_declares`
    /// - witness: `allocation_free_region::tests::a_why_outside_the_five_is_malformed`
    #[spec(ensures: |output| output.iter().any(|held| held == line.0))]
    fn costed(line: RustdocText<'_>) -> Vec<String>
    {
        [
            "Text.",
            "",
            "# Cost",
            line.0,
            "",
            "# Adequacy",
            "- allocation: none in steady state",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn each_why_declares()
    {
        for line in [
            "- allocation: the frame stack, setup",
            "- allocation: the refusal's text, error path.",
            "- allocation: the hover's text, diagnostic",
            "- allocation: the frames, growth within the declaration's depth",
            "- allocation: the verdict, durable by its content key",
            "- allocation: a table, of keys, setup",
        ] {
            assert_eq!(
                clause_of(&costed(RustdocText(line))),
                Clause::Declared,
                "{line}"
            );
        }
        assert_eq!(
            clause_of(&costed(RustdocText("- allocation: none in steady state."))),
            Clause::Steady
        );
    }

    #[test]
    fn a_why_outside_the_five_is_malformed()
    {
        for line in [
            "- allocation: the frames, growth within",
            "- allocation: the frames, growth within  ",
            "- allocation: the verdict, durable by",
            "- allocation: the frames, because it is simple",
            "- allocation: setup",
            "- allocation: , setup",
            "- allocation:",
        ] {
            assert_eq!(
                clause_of(&costed(RustdocText(line))),
                Clause::Malformed(line.to_owned()),
                "{line}"
            );
        }
    }

    #[test]
    fn only_the_cost_section_is_read()
    {
        let lines: Vec<String> = [
            "# Adequacy",
            "- allocation: none in steady state",
            "# Cost",
            "- time: linear",
        ]
        .map(str::to_owned)
        .to_vec();
        assert_eq!(clause_of(&lines), Clause::Silent);
        assert_eq!(clause_of(&[]), Clause::Silent);
    }

    #[test]
    fn a_pattern_takes_its_crates()
    {
        let whole = CratePattern::Exact("gandr_kernel_core".to_owned());
        let start = CratePattern::Prefix("gandr_kernel".to_owned());
        assert!(whole.matches(CrateName("gandr_kernel_core")).0);
        assert!(!whole.matches(CrateName("gandr_kernel_core_base")).0);
        assert!(start.matches(CrateName("gandr_kernel_core_base")).0);
        assert!(start.matches(CrateName("gandr_kernel")).0);
        assert!(!start.matches(CrateName("gandr_core_term")).0);
    }

    #[test]
    fn the_base_maps_each_function_to_its_body()
    {
        let text = [
            r#"{"record":"item","path":"case::kept","kind":"fn","body":"abcd1234-00ff"}"#,
            r#"{"record":"item","path":"case::Type::method","kind":"assoc-fn","body":"abcd1234-0a0b"}"#,
            r#"{"record":"item","path":"case::unhashed","kind":"fn"}"#,
            r#"{"record":"item","path":"case::Type","kind":"struct"}"#,
            r#"{"record":"use","item":"case::kept","names":[]}"#,
            "not json",
        ]
        .join("\n");
        let Maybe::Present(bodies) = base_bodies_of(RecordText(&text), BodyScheme("abcd1234"))
        else {
            panic!("a record of this scheme is a base");
        };
        assert_eq!(bodies.len(), 2);
        assert_eq!(
            bodies.get("case::kept").map(String::as_str),
            Some("abcd1234-00ff")
        );
        assert_eq!(
            bodies.get("case::Type::method").map(String::as_str),
            Some("abcd1234-0a0b")
        );
    }

    #[test]
    fn a_base_of_another_scheme_is_no_base()
    {
        let text = [
            r#"{"record":"item","path":"case::kept","kind":"fn","body":"ffff0000-00ff"}"#,
            r#"{"record":"item","path":"case::earlier","kind":"fn"}"#,
        ]
        .join("\n");
        assert!(matches!(
            base_bodies_of(RecordText(&text), BodyScheme("abcd1234")),
            Maybe::Absent(base_reading::Unread::OtherScheme)
        ));
        assert!(matches!(
            base_bodies_of(RecordText(&text), BodyScheme("ffff")),
            Maybe::Absent(base_reading::Unread::OtherScheme)
        ));
        let Maybe::Present(bodies) = base_bodies_of(RecordText("\n\n"), BodyScheme("abcd1234"))
        else {
            panic!("an empty record is a base with no function");
        };
        assert!(bodies.is_empty());
    }
}
