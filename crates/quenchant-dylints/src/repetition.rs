//! Where a body repeats: the functions that a loop, a role or a repeated
//! caller reaches, read over the crate's MIR.
//!
//! [`crate::cost_shape`] reads where an operation repeats inside its own body:
//! a loop, or a closure that an iterator calls once for each element. This
//! module reads where the body itself repeats. One of these reaches a
//! repeated function:
//!
//! - a loop: a cycle of a caller's MIR holds a call of the function;
//! - a role: the function implements `Iterator::next` or
//!   `DoubleEndedIterator::next_back`, which an iteration calls once for each
//!   element;
//! - a repeated caller: a function that repeats calls it.
//!
//! The table holds each repeated function of the crate with the first of these
//! that reaches it. The first question computes it once for each compilation,
//! from the optimized MIR of each function and closure. Every cost-shape pass
//! reads the one table through [`Repetitions`]. A call resolves to its
//! instance under the caller's own generic arguments. The calls of a closure
//! count as calls of the function that holds it, since the closure runs only
//! when that function runs.
//!
//! The table follows calls only in a crate that `repeated_call_crates` names
//! ([`Followed`]), by its whole name or by the start of it followed by `*`. A
//! crate outside the key reads no MIR for it, and its table is empty. A
//! consumer names its crates one by one, as each crate takes the refusals.
//!
//! # Boundaries
//!
//! - A loop of the MIR is a cycle, whatever its trip count: a call inside a
//!   loop over an array marks its callee too. Inside one body,
//!   [`crate::cost_shape`] reads such a loop through.
//! - An iterator adapter calls the closure that it takes from its own body, in
//!   a library crate, and this module does not read that body. Such a closure
//!   repeats by the reading of [`crate::cost_shape`] only, and a function that
//!   it calls does not repeat by it.
//! - A call through a trait object, a function pointer or a closure resolves to
//!   no function and marks nothing.
//! - The table holds the functions of the crate under the lint only. A caller
//!   in a different crate marks no function of this crate.

use alloc::sync::Arc;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::OnceLock;

use quenchant_shape::shape::Maybe;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LOCAL_CRATE;
use rustc_hir::def_id::LocalDefId;
use rustc_middle::mir::Body;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty;
use rustc_middle::ty::Instance;
use rustc_middle::ty::InstanceKind;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TypingEnv;
use rustc_span::Span;

use crate::crate_list;
use crate::crate_list::CratePattern;
use crate::crate_list::ListKey;
use crate::graph::tarjan_components;
use crate::semantic::CrateName;
use crate::semantic::FollowsCalls;
use crate::semantic::Vertex;

/// The key of the crates whose repetitions the machinery follows over calls.
const CALLS_KEY: ListKey = ListKey("repeated_call_crates");

/// The crates whose repetitions the machinery follows over calls, as the
/// configuration names them.
#[derive(Debug, Default)]
pub struct Followed
{
    /// The patterns of `repeated_call_crates`.
    crates: Vec<CratePattern>,
    /// Each entry of the key that names no crate, as a diagnostic.
    defects: Vec<String>,
}

impl Followed
{
    /// The crates that `repeated_call_crates` names.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: each entry of `repeated_call_crates` that names a crate, or
    ///   the start of one followed by `*`, becomes a pattern
    ///   ([`crate_list::read`]); a value that is no list, and each other entry,
    ///   becomes a defect. With no configuration or no key, no crate is
    ///   followed.
    /// - panics: none.
    /// - executable: none — the outcome is the configuration as read, which a
    ///   predicate could only read again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the call fixture names its crates; the refused
    ///   configuration names a number, an empty entry and `*`, each reported.
    /// - witness: `tests::ui_repeated_call`
    /// - witness: `tests::ui_repeated_call_refused`
    #[must_use]
    pub fn configured() -> Self
    {
        let mut defects = Vec::new();
        let crates = crate_list::read(CALLS_KEY, &mut defects);
        Self { crates, defects }
    }

    /// Whether the crate under the lint is one that the key names.
    ///
    /// # Specification
    /// - ensures: true exactly when one of the patterns takes the name of the
    ///   local crate.
    /// - panics: none.
    /// - executable: none — the outcome is the compiler's crate name against
    ///   the configuration.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the call fixtures follow the crates that the key
    ///   names, and the unfollowed fixture follows no call.
    /// - witness: `tests::ui_repeated_call`
    #[must_use]
    pub fn follows(
        &self,
        tcx: TyCtxt<'_>,
    ) -> FollowsCalls
    {
        let name = tcx.crate_name(LOCAL_CRATE);
        FollowsCalls(
            self.crates
                .iter()
                .any(|pattern| pattern.matches(CrateName(name.as_str())).0),
        )
    }

    /// Each entry of the key that names no crate, as a diagnostic.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub fn defects(&self) -> &[String]
    {
        &self.defects
    }
}

quenchant_shape::reason_enum! {
    /// A function can run once for each run of what calls it.
    pub mod repeat_reading {
        /// Why the function does not repeat.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Once {
            /// No loop, role or repeated caller of the crate reaches the
            /// function.
            Unreached,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A call can name something other than a function of this crate.
    mod callee_reading {
        /// Why the call names no function of this crate.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unlocal {
            /// The callee is a function pointer or a trait object.
            Indirect,
            /// The call is of a closure through `Fn`, `FnMut` or `FnOnce`.
            Closure,
            /// The callee resolves to no item.
            Unresolved,
            /// The callee belongs to another crate, or is a closure.
            Elsewhere,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A function can implement something other than a method that an
    /// iteration calls once for each element.
    mod role_reading {
        /// Why the function plays no repeated role.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NoRole {
            /// The function implements no trait method.
            Inherent,
            /// The trait method is not `Iterator::next` or
            /// `DoubleEndedIterator::next_back`.
            OtherMethod,
        }
    }
}

/// What makes a function repeat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Repeater
{
    /// A cycle of a caller's MIR holds this call of the function.
    Loop(Span),
    /// The function implements this trait method, which an iteration calls
    /// once for each element.
    Role(DefId),
    /// A function that repeats makes this call of the function.
    Caller(Span),
}

/// The repeated functions of one crate, which every cost-shape pass shares.
#[derive(Clone, Default)]
pub struct Repetitions
{
    /// The crates whose repetitions the table follows.
    followed: Arc<Followed>,
    /// The table, once the first question computes it.
    table: Arc<OnceLock<HashMap<LocalDefId, Repeater>>>,
}

impl Repetitions
{
    /// The repetitions of the crates that `followed` names.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub fn new(followed: Arc<Followed>) -> Self
    {
        Self {
            followed,
            table: Arc::default(),
        }
    }

    /// The crates whose repetitions the table follows.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub fn followed(&self) -> &Followed
    {
        &self.followed
    }

    /// What makes `function` repeat.
    ///
    /// # Specification
    /// - ensures: `Present` with the first loop, role or repeated caller that
    ///   reaches `function` ([`table`]) in a crate that the key names
    ///   ([`Followed::follows`]); `Absent(Unreached)` otherwise. The first
    ///   question of a compilation computes the table, and each later one reads
    ///   it; a crate outside the key computes an empty one.
    /// - panics: none.
    /// - executable: none — the outcome is the table of the whole crate, which
    ///   a predicate could only compute again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse a throwaway in a function that a
    ///   loop calls, in one that a repeated caller calls and in an
    ///   implementation of `Iterator::next`, and accept the same throwaway in a
    ///   function that only straight code calls and in a crate outside the key.
    /// - witness: `tests::ui_repeated_call`
    pub fn of(
        &self,
        tcx: TyCtxt<'_>,
        function: LocalDefId,
    ) -> Maybe<Repeater, repeat_reading::Once>
    {
        self.table
            .get_or_init(|| {
                if self.followed.follows(tcx).0 {
                    table(tcx)
                }
                else {
                    HashMap::new()
                }
            })
            .get(&function)
            .copied()
            .map_or(
                Maybe::Absent(repeat_reading::Once::Unreached),
                Maybe::Present,
            )
    }
}

/// The repeated functions of the crate, each with the first loop, role or
/// repeated caller that reaches it.
///
/// # Specification
/// - ensures: a function is a key exactly when a cycle of the MIR of some
///   function or closure of the crate holds a call that resolves to it
///   ([`local_callee`]), when it plays a role ([`role`]), or when a key's MIR,
///   or that of a closure the key holds, holds a call that resolves to it. Its
///   value is a loop's call, else its role, else a repeated caller's call: the
///   reading marks every loop and role before it follows a caller.
/// - panics: none.
/// - executable: none — the outcome is a fixed point over the MIR of the whole
///   crate, which a predicate could only compute again.
///
/// # Termination
/// - reason: no recursion; the propagation is a loop over a work list.
/// - measure: the functions not yet in the table.
/// - boundedness: a function enters the work list only when it enters the
///   table, so each one is read once.
/// - input recursion: none.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures reach a throwaway by a loop of the caller,
///   by a chain of two callers, by a role and through a closure's loop, and
///   reach none from straight code.
/// - witness: `tests::ui_cost_shape`
fn table(tcx: TyCtxt<'_>) -> HashMap<LocalDefId, Repeater>
{
    let mut calls: HashMap<LocalDefId, Vec<(LocalDefId, Span)>> = HashMap::new();
    let mut repeated: HashMap<LocalDefId, Repeater> = HashMap::new();
    let mut pending: Vec<LocalDefId> = Vec::new();
    let mut scratch = Scratch::default();
    for owner in tcx.hir_body_owners() {
        if !matches!(
            tcx.def_kind(owner),
            DefKind::Fn | DefKind::AssocFn | DefKind::Closure
        ) || !tcx.is_mir_available(owner.to_def_id())
        {
            continue;
        }
        let holder = holder(tcx, owner);
        if let Maybe::Present(method) = role(tcx, owner)
            && let Entry::Vacant(slot) = repeated.entry(owner)
        {
            slot.insert(Repeater::Role(method));
            pending.push(owner);
        }
        let body = tcx.optimized_mir(owner);
        let typing_env = TypingEnv::post_analysis(tcx, owner);
        cyclic_blocks(body, &mut scratch);
        for (block, data) in body.basic_blocks.iter_enumerated() {
            let Some(terminator) = data.terminator.as_ref()
            else {
                continue;
            };
            let TerminatorKind::Call {
                ref func, fn_span, ..
            } = terminator.kind
            else {
                continue;
            };
            let Maybe::Present(callee) =
                local_callee(tcx, typing_env, func.ty(&body.local_decls, tcx))
            else {
                continue;
            };
            if scratch.cyclic.get(block.as_usize()).copied() == Some(Cycle::On)
                && let Entry::Vacant(slot) = repeated.entry(callee)
            {
                slot.insert(Repeater::Loop(fn_span));
                pending.push(callee);
            }
            calls.entry(holder).or_default().push((callee, fn_span));
        }
    }
    while let Some(caller) = pending.pop() {
        for &(callee, call) in calls.get(&caller).into_iter().flatten() {
            if let Entry::Vacant(slot) = repeated.entry(callee) {
                slot.insert(Repeater::Caller(call));
                pending.push(callee);
            }
        }
    }
    repeated
}

/// The function that holds a body: the body's own function, or the function
/// that holds a closure.
///
/// # Specification
/// - ensures: the root of `body`'s type-checking, which is `body` itself for a
///   function and the enclosing function for a closure.
/// - panics: none.
/// - executable: none — the outcome is the compiler's definition table.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures reach a throwaway through a loop inside a
///   closure, which counts against the function that holds the closure.
/// - witness: `tests::ui_cost_shape`
fn holder(
    tcx: TyCtxt<'_>,
    body: LocalDefId,
) -> LocalDefId
{
    tcx.typeck_root_def_id(body.to_def_id())
        .as_local()
        .unwrap_or(body)
}

/// The function of this crate that a call resolves to.
///
/// # Specification
/// - ensures: `Present` when `callee` is a function item that resolves under
///   `typing_env` to an item of this crate other than a closure;
///   `Absent(Indirect)` for a function pointer or a trait object;
///   `Absent(Closure)` for a call through `Fn`, `FnMut` or `FnOnce`;
///   `Absent(Unresolved)` when no instance resolves; `Absent(Elsewhere)`
///   otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's instance resolution.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a local function, a local method, a
///   trait method on a concrete type and a closure from inside loops.
/// - witness: `tests::ui_cost_shape`
fn local_callee<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    callee: Ty<'tcx>,
) -> Maybe<LocalDefId, callee_reading::Unlocal>
{
    let ty::FnDef(def_id, args) = *callee.kind()
    else {
        return Maybe::Absent(callee_reading::Unlocal::Indirect);
    };
    if tcx
        .trait_of_assoc(def_id)
        .is_some_and(|owner| tcx.fn_trait_kind_from_def_id(owner).is_some())
    {
        return Maybe::Absent(callee_reading::Unlocal::Closure);
    }
    let Some(args) = args.no_bound_vars()
    else {
        return Maybe::Absent(callee_reading::Unlocal::Unresolved);
    };
    let args = tcx.erase_and_anonymize_regions(args);
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`InstanceKind` is rustc's instance kind, whose variants move with the pinned \
                  nightly; only an item names a function"
    )]
    let target = match Instance::try_resolve(tcx, typing_env, def_id, args) {
        Ok(Some(instance)) => match instance.def {
            InstanceKind::Item(target) => target,
            _ => return Maybe::Absent(callee_reading::Unlocal::Unresolved),
        },
        Ok(None) | Err(_) => return Maybe::Absent(callee_reading::Unlocal::Unresolved),
    };
    match target.as_local() {
        Some(local) if !tcx.is_closure_like(target) => Maybe::Present(local),
        Some(_) | None => Maybe::Absent(callee_reading::Unlocal::Elsewhere),
    }
}

/// The trait method a function implements that an iteration calls once for
/// each element.
///
/// # Specification
/// - ensures: `Present` with the declaration when `function` implements
///   `Iterator::next` or `DoubleEndedIterator::next_back`; `Absent(Inherent)`
///   when it implements no trait method; `Absent(OtherMethod)` otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's association of an
///   implementation with its declaration.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a throwaway in an implementation of
///   `Iterator::next` and accept the same in an inherent method of the same
///   type.
/// - witness: `tests::ui_cost_shape`
fn role(
    tcx: TyCtxt<'_>,
    function: LocalDefId,
) -> Maybe<DefId, role_reading::NoRole>
{
    let Some(declaration) = tcx.trait_item_of(function.to_def_id())
    else {
        return Maybe::Absent(role_reading::NoRole::Inherent);
    };
    let name = tcx.item_name(declaration);
    let iterates = tcx
        .trait_of_assoc(declaration)
        .and_then(|owner| tcx.get_diagnostic_name(owner))
        .is_some_and(|owner| {
            matches!(
                (owner.as_str(), name.as_str()),
                ("Iterator", "next") | ("DoubleEndedIterator", "next_back")
            )
        });
    if iterates {
        Maybe::Present(declaration)
    }
    else {
        Maybe::Absent(role_reading::NoRole::OtherMethod)
    }
}

/// Whether a block lies on a cycle of the control flow of its body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Cycle
{
    /// The block lies on a cycle: a loop can run it again.
    On,
    /// No path leads from the block back to it.
    Off,
}

/// The buffers that [`table`] reuses for each body that it reads.
#[derive(Default)]
struct Scratch
{
    /// The successors of each block, in block order.
    adjacency: Vec<Vec<Vertex>>,
    /// The cycle of each block, in block order.
    cyclic: Vec<Cycle>,
}

/// Mark the blocks of a body that lie on a cycle of its control flow.
///
/// # Specification
/// - ensures: `scratch.cyclic` holds one entry per block, in block order, `On`
///   exactly when the block lies in a strongly connected component of more than
///   one block or has an edge to itself; the edges are each terminator's
///   successors, its unwind edges included.
/// - ensures: the buffers of `scratch` keep their capacity from body to body.
/// - panics: none.
/// - executable: none — the outcome is [`tarjan_components`] over the body's
///   edges, which a predicate could only compute again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures call a function in a `for`, a `while` and a
///   `loop` body, which are cycles, and after a loop, which is none.
/// - witness: `tests::ui_cost_shape`
fn cyclic_blocks(
    body: &Body<'_>,
    scratch: &mut Scratch,
)
{
    let count = body.basic_blocks.len();
    scratch.adjacency.truncate(count);
    scratch.adjacency.resize_with(count, Vec::new);
    for (successors, data) in scratch.adjacency.iter_mut().zip(body.basic_blocks.iter()) {
        successors.clear();
        if let Some(terminator) = data.terminator.as_ref() {
            successors.extend(
                terminator
                    .successors()
                    .map(|successor| Vertex(successor.as_usize())),
            );
        }
    }
    scratch.cyclic.clear();
    scratch.cyclic.resize(count, Cycle::Off);
    for component in tarjan_components(&scratch.adjacency) {
        let on_cycle = match *component.as_slice() {
            [single] => scratch
                .adjacency
                .get(single.0)
                .is_some_and(|successors| successors.contains(&single)),
            _ => true,
        };
        if on_cycle {
            for vertex in component {
                if let Some(slot) = scratch.cyclic.get_mut(vertex.0) {
                    *slot = Cycle::On;
                }
            }
        }
    }
}
