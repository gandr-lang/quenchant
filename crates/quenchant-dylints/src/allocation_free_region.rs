//! Refuse an allocation reachable from a function marked allocation-free in
//! steady state.
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
//!
//! # Items
//!
//! - [`ALLOCATION_FREE_REGION`]: the lint.
//! - [`marker`]: the `# Cost` reader.
//! - [`Summaries`]: the per-instance reach of every body read.
//! - [`AllocationFreeRegion`]: the pass.

use std::collections::HashMap;
use std::collections::HashSet;

use anodized::spec;
use clippy_utils::diagnostics::span_lint_hir_and_then;
use quenchant_shape::shape::Maybe;
use rustc_hir::HirId;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty;
use rustc_middle::ty::EarlyBinder;
use rustc_middle::ty::GenericArgs;
use rustc_middle::ty::Instance;
use rustc_middle::ty::InstanceKind;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TypingEnv;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;

use crate::cost_shape::Ownership;
use crate::cost_shape::heap_ownership;
use crate::walk_summary::doc_lines;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses, in a function whose rustdoc carries a `# Cost` section with the clause `- allocation: none in steady state`, every call from which a path reaches an allocation: the allocator's entry points through any body the build can read, a body it cannot read whose signature returns or takes by `&mut` a value owning a heap allocation, and a call through a trait object, a function pointer or an unmarked trait method of a type parameter. An implementation of a trait method whose declaration carries the clause is held to it too.
    ///
    /// ### Why is this bad?
    ///
    /// The function promises that once warmed up it allocates nothing; an allocation on any path breaks the promise on the path the inputs select, and pays the allocator on every item that takes it.
    ///
    /// ### Accepted forms
    ///
    /// A call into `core`, a call that returns `!`, a call that reaches no allocation, and a call under `#[expect(allocation_free_region, reason = "...")]` at the site. A value's destructor is not followed.
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
    "a function marked allocation-free in steady state reaches an allocation"
}

/// The heading of the section that states a function's cost.
const HEADING: &str = "# Cost";

/// The clause inside that section that marks the region.
const CLAUSE: &str = "- allocation: none in steady state";

/// The allocator entry points of the `alloc` crate, by path.
const ALLOCATOR_ENTRIES: [&str; 4] = [
    "alloc::alloc::alloc",
    "alloc::alloc::alloc_zeroed",
    "alloc::alloc::realloc",
    "alloc::alloc::exchange_malloc",
];

/// Whether a function's rustdoc marks it allocation-free in steady state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Marker
{
    /// A `# Cost` section carries the clause.
    Marked,
    /// It does not.
    Unmarked,
}

/// Read a function's `# Cost` section.
///
/// # Specification
/// - ensures: `Marked` exactly when a line of the rustdoc is `# Cost` and a
///   later line before the next heading is the clause, a trailing period
///   allowed.
/// - panics: none.
/// - executable: none — the outcome is decided by scanning the item's rustdoc
///   lines; a predicate could only repeat that scan.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures mark functions, a trait method and an
///   implementation, and leave a `# Cost` section with another clause unmarked.
/// - witness: `tests::ui_cost_shape`
pub fn marker(
    tcx: TyCtxt<'_>,
    def_id: DefId,
) -> Marker
{
    let lines = doc_lines(tcx, def_id);
    let Some(start) = lines.iter().position(|line| line == HEADING)
    else {
        return Marker::Unmarked;
    };
    let marked = lines
        .iter()
        .skip(start.saturating_add(1))
        .take_while(|line| !line.starts_with('#'))
        .any(|line| line.trim_end_matches('.') == CLAUSE);
    if marked {
        Marker::Marked
    }
    else {
        Marker::Unmarked
    }
}

/// What reaches an allocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause
{
    /// A path reaches the allocator's entry point.
    Allocator,
    /// A body the build cannot read may allocate by its signature.
    Signature,
    /// A call may reach any implementation.
    Dynamic,
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
    /// - ensures: the reach of `instance` is recorded afterwards, with that of
    ///   every body it reaches; a body reached again while its own reading is
    ///   pending counts as free, so a call cycle ends.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse an allocation two calls deep and
    ///   through a closure, and accept a recursive pair that allocates nothing.
    /// - witness: `tests::ui_cost_shape`
    #[spec(ensures: |_| self.0.contains_key(&instance))]
    pub fn of(
        &mut self,
        tcx: TyCtxt<'tcx>,
        typing_env: TypingEnv<'tcx>,
        instance: Instance<'tcx>,
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
            match read_body(tcx, typing_env, &self.0, &reading, current) {
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
            EarlyBinder::bind(func.ty(&body.local_decls, tcx)),
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
///   allocates, or `Free`.
/// - panics: none.
/// - executable: none — the outcome is a scan over the body's calls against the
///   summaries; a predicate could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse an allocation two calls deep.
/// - witness: `tests::ui_cost_shape`
fn read_body<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    done: &HashMap<Instance<'tcx>, Reach>,
    reading: &HashSet<Instance<'tcx>>,
    instance: Instance<'tcx>,
) -> Reading<'tcx>
{
    let mut needs = Vec::new();
    let mut first = Reach::Free;
    for (span, _, call) in calls(tcx, typing_env, instance) {
        let reach = match call {
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
///   points; `Allocates(Dynamic)` for a call through a function pointer, a
///   trait object or an unresolved trait method whose declaration is unmarked;
///   `Free` for an unresolved trait method whose declaration is marked; `Body`
///   for a resolved instance whose MIR is available; `Allocates(Signature)` or
///   `Free` by [`signature_allocates`] otherwise.
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
    let Ok(Some(instance)) = Instance::try_resolve(tcx, typing_env, def_id, args)
    else {
        return trait_call(tcx, def_id);
    };
    match instance.def {
        InstanceKind::Virtual(..) => trait_call(tcx, def_id),
        InstanceKind::DropGlue(..)
        | InstanceKind::AsyncDropGlue(..)
        | InstanceKind::Intrinsic(_) => Call::Free,
        InstanceKind::Item(item)
            if in_core(tcx, item) == Allocation::Never
                && instance.args.types().next().is_none() =>
        {
            Call::Free
        },
        InstanceKind::Item(item) if tcx.is_mir_available(item) => Call::Body(instance),
        InstanceKind::Item(item) => signature_allocates(tcx, typing_env, item),
        InstanceKind::VTableShim(_)
        | InstanceKind::ReifyShim(..)
        | InstanceKind::FnPtrShim(..)
        | InstanceKind::ClosureOnceShim { .. }
        | InstanceKind::ConstructCoroutineInClosureShim { .. }
        | InstanceKind::ThreadLocalShim(_)
        | InstanceKind::FutureDropPollShim(..)
        | InstanceKind::AsyncDropGlueCtorShim(..)
        | InstanceKind::CloneShim(..)
        | InstanceKind::FnPtrAddrShim(..) => Call::Body(instance),
    }
}

/// Classify a call that resolves to no concrete body.
///
/// # Specification
/// - ensures: `Free` when the callee's trait method declaration is marked;
///   `Allocates(Dynamic)` otherwise.
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
) -> Call<'_>
{
    if marker(tcx, def_id) == Marker::Marked {
        Call::Free
    }
    else {
        Call::Allocates(Cause::Dynamic)
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

/// The marked regions whose calls this lint reads.
#[derive(Default)]
#[repr(transparent)]
pub struct AllocationFreeRegion<'tcx>
{
    /// The summaries computed so far, shared by every region of the crate.
    summaries: Summaries<'tcx>,
}

impl_lint_pass!(AllocationFreeRegion<'_> => [ALLOCATION_FREE_REGION]);

/// Whether a function is held to the allocation-free clause.
///
/// # Specification
/// - ensures: `Marked` when the function's own rustdoc carries the clause, or
///   when it implements a trait method whose declaration carries it; `Unmarked`
///   otherwise.
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
) -> Marker
{
    if marker(tcx, def_id.to_def_id()) == Marker::Marked {
        return Marker::Marked;
    }
    tcx.trait_item_of(def_id.to_def_id())
        .map_or(Marker::Unmarked, |declaration| marker(tcx, declaration))
}

impl<'tcx> LateLintPass<'tcx> for AllocationFreeRegion<'tcx>
{
    /// Refuse every call of a marked function that reaches an allocation.
    ///
    /// # Specification
    /// - ensures: for a function [`held`] to the clause, one refusal per call
    ///   in its MIR, in block order, whose callee is not free, reported at the
    ///   call under the call's own lint level; nothing for any other item.
    /// - panics: none.
    /// - executable: none — the outcome is emitted diagnostics.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse direct, nested, dynamic and
    ///   signature-read allocations and a marked trait method's implementation,
    ///   and accept marked bodies reaching only `core`, panics and marked trait
    ///   methods, and a site under `expect`.
    /// - witness: `tests::ui_cost_shape`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let tcx = cx.tcx;
        for owner in tcx.hir_body_owners() {
            if !matches!(tcx.def_kind(owner), DefKind::Fn | DefKind::AssocFn) {
                continue;
            }
            if held(tcx, owner) == Marker::Unmarked {
                continue;
            }
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
                    Call::Body(callee) => self.summaries.of(tcx, typing_env, callee),
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
                    span,
                    "this call can allocate inside a function marked allocation-free in steady \
                     state",
                    |diag| {
                        let reason = match witness.cause {
                            Cause::Allocator => "a call into the allocator",
                            Cause::Signature => {
                                "a call whose body is unavailable and whose signature hands \
                                 back or writes a value that owns heap memory"
                            },
                            Cause::Dynamic => {
                                "a call that can reach any implementation, none of them held \
                                 to the clause"
                            },
                        };
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
                            "reserve or build what the region needs before it is entered, or \
                             mark the callee with the same clause",
                        );
                    },
                );
            }
        }
    }
}
