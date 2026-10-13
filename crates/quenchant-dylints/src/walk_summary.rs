//! What a function body walks, read from MIR and followed through its calls.
//!
//! A clause that calls a function costs what that function costs. The
//! clause-cost gate therefore summarizes every body a clause reaches: the
//! positions — the parameters of a function, the captured values of a closure
//! — whose reachable state the body walks, directly through the standard
//! library or through another summarized body. Following stops at a function
//! whose rustdoc carries a `# Clause cost` section with a `- bound:` clause:
//! its author states the bound a clause may rely on, and the section documents
//! it where readers look.
//!
//! Bodies from other crates are read from their metadata, which carries every
//! function's MIR only when the whole graph is built with
//! `-Zalways-encode-mir`; the gate's invocation adds `-Zinline-mir=no` so that
//! standard-library calls survive as calls whatever the profile. The rustdoc
//! that marks a bound crosses crates for exported items, which are the only
//! foreign functions a clause can call.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use anodized::spec;
use quenchant_shape::shape::Maybe;
use rustc_hir::Attribute;
use rustc_hir::def::DefKind;
use rustc_hir::def_id::DefId;
use rustc_middle::mir::AggregateKind;
use rustc_middle::mir::Body;
use rustc_middle::mir::Local;
use rustc_middle::mir::Operand;
use rustc_middle::mir::Place;
use rustc_middle::mir::ProjectionElem;
use rustc_middle::mir::Rvalue;
use rustc_middle::mir::StatementKind;
use rustc_middle::mir::TerminatorKind;
use rustc_middle::ty;
use rustc_middle::ty::GenericArgsRef;
use rustc_middle::ty::Instance;
use rustc_middle::ty::InstanceKind;
use rustc_middle::ty::Ty;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::TypeVisitableExt as _;
use rustc_middle::ty::TypingEnv;
use rustc_span::Span;
use rustc_span::Spanned;
use rustc_span::Symbol;
use rustc_span::sym;

quenchant_shape::reason_enum! {
    /// A position can name no item of a list.
    mod position_reading {
        /// Why the position selects nothing.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The list is shorter than the position.
            OutOfRange,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A type can hold a bounded number of elements.
    mod container_reading {
        /// Why the type is not a growable container.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Bounded {
            /// The type is not one of the standard sequences or collections.
            NotAContainer,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A type can be something other than an iterator.
    mod iterator_reading {
        /// Why the type is not an iterator.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAnIterator {
            /// The type does not implement `Iterator`, or the trait is not
            /// resolvable.
            NoImplementation,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A standard-library call can leave its receiver's elements unread.
    mod library_reading {
        /// Why the call is not a walk.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAWalk {
            /// The receiver is fixed-size, an iterator already, or not a
            /// growable container.
            Bounded,
            /// The method reads a bounded part of the container.
            BoundedMethod,
        }
    }
}

/// The heading of the section that states a function's bound.
const HEADING: &str = "# Clause cost";

/// The clause inside that section that states the bound.
const BOUND: &str = "- bound:";

/// The crates whose functions are classified by name rather than followed.
pub const LIBRARY_CRATES: [&str; 3] = ["core", "alloc", "std"];

/// The crate whose functions instrument specifications.
pub const INSTRUMENTATION_CRATE: &str = "anodized";

/// The instrumentation entry point that runs the function's own body.
const BODY_ENTRY: &str = "eval_once";

/// How a body walks the state one of its positions reaches.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Walk
{
    /// A method starts an iterator over the state.
    Iterates,
    /// A sequence is searched or order-checked element by element.
    Searches,
    /// A collection is copied.
    Copies,
    /// Two collections are compared element by element.
    Compares,
    /// The state is handed to a call that cannot be resolved to a body.
    Unresolved,
    /// The state is handed to a body that the dependency's metadata does not
    /// carry.
    Unreadable,
}

/// A parameter of a function body, or a captured value of a closure body.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Position(u32);

impl Position
{
    /// The item of `items` this position names.
    ///
    /// # Specification
    /// - ensures: `Present` with the item at this position when `items` reaches
    ///   it; `Absent` otherwise.
    /// - panics: none.
    /// - executable: none — the outcome is one checked slice lookup.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture reports a helper through the receiver
    ///   and through a later argument, beside the same helper given unrooted
    ///   arguments at both positions.
    /// - witness: `tests::ui_spec_cost`
    pub fn select<T>(
        self,
        items: &[T],
    ) -> Maybe<&T, position_reading::Missing>
    {
        usize::try_from(self.0)
            .ok()
            .and_then(|index| items.get(index))
            .map_or(
                Maybe::Absent(position_reading::Missing::OutOfRange),
                Maybe::Present,
            )
    }
}

/// Where a walk happens: its kind and its source.
#[derive(Clone, Copy, Debug)]
pub struct Witness
{
    /// How the state is walked.
    pub walk: Walk,
    /// The call that walks it, in whichever body it sits.
    pub span: Span,
}

/// The positions a body walks, each with the first walk found.
#[derive(Clone, Debug, Default)]
#[repr(transparent)]
pub struct Summary(BTreeMap<Position, Witness>);

impl Summary
{
    /// The walked positions in ascending order.
    ///
    /// # Specification
    /// trivial.
    pub fn walks(&self) -> impl Iterator<Item = (Position, Witness)> + '_
    {
        self.0
            .iter()
            .map(|(&position, &witness)| (position, witness))
    }

    /// Record a walk of `position` unless one is already recorded.
    ///
    /// # Specification
    /// - ensures: `position` is recorded afterwards, with its earlier witness
    ///   when it had one.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture's helpers walk the same parameter
    ///   twice and are reported once, at the first walk.
    /// - witness: `tests::ui_spec_cost`
    #[spec(ensures: |_| self.0.contains_key(&position))]
    fn record(
        &mut self,
        position: Position,
        witness: Witness,
    )
    {
        self.0.entry(position).or_insert(witness);
    }
}

/// Whether a function states the bound a clause may rely on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section
{
    /// The rustdoc has a `# Clause cost` section with a non-empty `- bound:`.
    Bounded,
    /// The rustdoc has the heading without a non-empty `- bound:`.
    Malformed,
    /// The rustdoc has no such heading.
    Absent,
}

/// Read a function's `# Clause cost` section.
///
/// # Specification
/// - ensures: [`Section::Bounded`] when a line of the rustdoc is the heading
///   and a later line before the next heading starts with `- bound:` followed
///   by text; [`Section::Malformed`] when the heading is present without such a
///   line; [`Section::Absent`] without the heading.
/// - panics: none.
/// - executable: none — the outcome is decided by scanning the item's rustdoc
///   lines; a predicate could only repeat that scan.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture marks a local combinator, a foreign
///   combinator and a trait method, and leaves one heading without its bound,
///   which is reported at its definition.
/// - witness: `tests::ui_spec_cost`
pub fn section(
    tcx: TyCtxt<'_>,
    def_id: DefId,
) -> Section
{
    #[expect(
        deprecated,
        reason = "the section is read from every doc line in order; `find_attr!` selects one parsed attribute"
    )]
    let attrs = tcx.get_all_attrs(def_id);
    let lines: Vec<String> = attrs
        .iter()
        .filter_map(Attribute::doc_str)
        .flat_map(|doc| {
            doc.as_str()
                .lines()
                .map(|line| line.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    let Some(start) = lines.iter().position(|line| line == HEADING)
    else {
        return Section::Absent;
    };
    let bounded = lines
        .iter()
        .skip(start)
        .skip(1)
        .take_while(|line| !line.starts_with('#'))
        .any(|line| {
            line.strip_prefix(BOUND)
                .is_some_and(|text| !text.trim().is_empty())
        });
    if bounded {
        Section::Bounded
    }
    else {
        Section::Malformed
    }
}

/// What a call reaches once its callee is resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Callee
{
    /// A standard-library function, classified by name.
    Library(DefId),
    /// A specification-instrumentation function.
    Instrumentation,
    /// A function whose rustdoc states its bound.
    Bounded,
    /// A body that can be summarized.
    Body(DefId),
    /// A trait method that resolves to no single body here.
    Unresolved(DefId),
    /// A function whose body the dependency's metadata does not carry.
    Unreadable(DefId),
    /// A shim, an intrinsic or a closure call, whose work is accounted where
    /// the closure is built.
    Opaque,
}

/// Resolve a call's callee and decide how a walk through it is found.
///
/// # Specification
/// - ensures: resolves `def_id` with `args` in `typing_env`; a resolved
///   closure, shim or intrinsic, and a failed resolution, are
///   [`Callee::Opaque`]; otherwise, by the target's crate and rustdoc, in this
///   order: [`Callee::Library`] for `core`, `alloc` and `std`;
///   [`Callee::Instrumentation`] for `anodized`; [`Callee::Bounded`] for a
///   [`Section::Bounded`] target; [`Callee::Unresolved`] for a trait method
///   without a single body; [`Callee::Body`] when the target's MIR is
///   available; [`Callee::Unreadable`] otherwise.
/// - panics: none.
/// - executable: none — the outcome is the compiler's instance resolution and
///   metadata queries, which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture calls a local helper, a foreign helper, a
///   marked combinator, a trait method on a type parameter, a marked trait
///   method and standard-library methods from clauses; each is reported or
///   accepted by its class.
/// - witness: `tests::ui_spec_cost`
pub fn classify<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    def_id: DefId,
    args: GenericArgsRef<'tcx>,
) -> Callee
{
    if !matches!(tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn)
        || tcx.generics_of(def_id).count() != args.len()
    {
        return Callee::Opaque;
    }
    let args = tcx.erase_and_anonymize_regions(args);
    let (target, resolved) = match Instance::try_resolve(tcx, typing_env, def_id, args) {
        | Ok(Some(instance)) => match instance.def {
            | InstanceKind::Item(target) if tcx.is_closure_like(target) => return Callee::Opaque,
            | InstanceKind::Item(target) => (target, true),
            | InstanceKind::Virtual(target, _) => (target, false),
            | _ => return Callee::Opaque,
        },
        | Ok(None) => (def_id, false),
        | Err(_) => return Callee::Opaque,
    };
    let krate = tcx.crate_name(target.krate);
    if LIBRARY_CRATES.contains(&krate.as_str()) {
        return Callee::Library(target);
    }
    if krate.as_str() == INSTRUMENTATION_CRATE {
        return Callee::Instrumentation;
    }
    if section(tcx, target) == Section::Bounded {
        return Callee::Bounded;
    }
    if !resolved {
        return Callee::Unresolved(target);
    }
    if tcx.is_mir_available(target) {
        Callee::Body(target)
    }
    else {
        Callee::Unreadable(target)
    }
}

/// Which kind of growable container a type is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Container
{
    /// A slice, `str`, `Vec`, `VecDeque` or `String`: searched element by
    /// element.
    Sequence,
    /// A standard map, set, heap or list: searched by key, if at all.
    Collection,
}

/// Classify a type as a growable container.
///
/// # Specification
/// - ensures: [`Container::Sequence`] for slices, `str`, `Vec`, `VecDeque` and
///   `String`; [`Container::Collection`] for `BTreeMap`, `BTreeSet`, `HashMap`,
///   `HashSet`, `BinaryHeap` and `LinkedList`; `Absent` otherwise, arrays
///   included.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture searches a vector and a set, copies a
///   vector and a map, and compares vectors and arrays; only the growable forms
///   are reported.
/// - witness: `tests::ui_spec_cost`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => !held.is_array(),
    Maybe::Absent(_) => true,
})]
pub fn container<'tcx>(
    tcx: TyCtxt<'tcx>,
    held: Ty<'tcx>,
) -> Maybe<Container, container_reading::Bounded>
{
    if held.is_slice() || held.is_str() {
        return Maybe::Present(Container::Sequence);
    }
    let ty::Adt(definition, _) = *held.kind()
    else {
        return Maybe::Absent(container_reading::Bounded::NotAContainer);
    };
    let name = tcx.get_diagnostic_name(definition.did());
    match name.as_ref().map(Symbol::as_str) {
        | Some("Vec" | "VecDeque" | "String") => Maybe::Present(Container::Sequence),
        | Some("BTreeMap" | "BTreeSet" | "HashMap" | "HashSet" | "BinaryHeap" | "LinkedList") => {
            Maybe::Present(Container::Collection)
        },
        | _ => Maybe::Absent(container_reading::Bounded::NotAContainer),
    }
}

/// Whether a type implements `Iterator`.
///
/// # Specification
/// - ensures: `Present` exactly when the standard `Iterator` trait resolves and
///   the type implements it in `typing_env`.
/// - panics: none.
/// - executable: none — the predicate would repeat the one trait query the body
///   makes.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture chains adapters after a refused `iter()`,
///   which report once, at the call that starts the iterator.
/// - witness: `tests::ui_spec_cost`
pub fn iterator<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    held: Ty<'tcx>,
) -> Maybe<(), iterator_reading::NotAnIterator>
{
    let implements = tcx
        .get_diagnostic_item(sym::Iterator)
        .is_some_and(|trait_id| {
            clippy_utils::ty::implements_trait_with_env(tcx, typing_env, held, trait_id, None, &[])
        });
    if implements {
        Maybe::Present(())
    }
    else {
        Maybe::Absent(iterator_reading::NotAnIterator::NoImplementation)
    }
}

/// Classify a standard-library call by its name, its receiver and its result.
///
/// # Specification
/// - ensures: `Absent` when the receiver, after references, is an array, an
///   iterator, or not a growable container; otherwise [`Walk::Iterates`] when
///   the result is an iterator, [`Walk::Searches`] for `contains` and the
///   `is_sorted` family on a sequence, [`Walk::Copies`] for `clone`, `to_owned`
///   and `to_vec`, [`Walk::Compares`] for `eq` and `ne`, and `Absent` for every
///   other method.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture's helpers iterate, search, copy and
///   compare vectors, beside bounded reads of the same vectors and walks of
///   arrays, which pass.
/// - witness: `tests::ui_spec_cost`
#[spec(ensures: |output| match output {
    Maybe::Present(_) => !receiver.peel_refs().is_array(),
    Maybe::Absent(_) => true,
})]
pub fn library_walk<'tcx>(
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    name: Symbol,
    receiver: Ty<'tcx>,
    result: Ty<'tcx>,
) -> Maybe<Walk, library_reading::NotAWalk>
{
    let held = receiver.peel_refs();
    if held.is_array() || matches!(iterator(tcx, typing_env, held), Maybe::Present(())) {
        return Maybe::Absent(library_reading::NotAWalk::Bounded);
    }
    let Maybe::Present(kind) = container(tcx, held)
    else {
        return Maybe::Absent(library_reading::NotAWalk::Bounded);
    };
    if matches!(iterator(tcx, typing_env, result), Maybe::Present(())) {
        return Maybe::Present(Walk::Iterates);
    }
    match (kind, name.as_str()) {
        | (Container::Sequence, "contains" | "is_sorted" | "is_sorted_by" | "is_sorted_by_key") => {
            Maybe::Present(Walk::Searches)
        },
        | (_, "clone" | "to_owned" | "to_vec") => Maybe::Present(Walk::Copies),
        | (_, "eq" | "ne") => Maybe::Present(Walk::Compares),
        | _ => Maybe::Absent(library_reading::NotAWalk::BoundedMethod),
    }
}

/// The summaries computed so far in one compilation.
#[derive(Default)]
#[repr(transparent)]
pub struct Summaries(HashMap<DefId, Summary>);

/// The outcome of reading one body.
enum Reading
{
    /// Every body the reading depends on was summarized.
    Ready(Summary),
    /// These bodies must be summarized first.
    Needs(Vec<DefId>),
}

impl Summaries
{
    /// The summary of `def_id`, computing it and every body it reaches.
    ///
    /// # Specification
    /// - ensures: the summary of `def_id` is recorded afterwards, with that of
    ///   every body it reaches; a body reached again while its own reading is
    ///   pending counts as walking nothing, so a call cycle ends.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture reports walks two calls deep, through
    ///   a closure and through another crate, and accepts a chain that stops at
    ///   a marked combinator.
    /// - witness: `tests::ui_spec_cost`
    #[spec(ensures: |_| self.0.contains_key(&def_id))]
    pub fn of(
        &mut self,
        tcx: TyCtxt<'_>,
        def_id: DefId,
    ) -> Summary
    {
        let mut pending = vec![def_id];
        let mut reading = HashSet::new();
        while let Some(&current) = pending.last() {
            if self.0.contains_key(&current) {
                pending.pop();
                reading.remove(&current);
                continue;
            }
            match read_body(tcx, &self.0, &reading, current) {
                | Reading::Ready(summary) => {
                    self.0.insert(current, summary);
                    pending.pop();
                    reading.remove(&current);
                },
                | Reading::Needs(callees) => {
                    reading.insert(current);
                    pending.extend(callees);
                },
            }
        }
        self.0.get(&def_id).cloned().unwrap_or_default()
    }
}

/// The positions each local of a body reaches.
#[derive(Default)]
#[repr(transparent)]
struct Reach(HashMap<Local, BTreeSet<Position>>);

impl Reach
{
    /// The positions a place reaches.
    ///
    /// # Specification
    /// - ensures: in a closure body, a field of the environment local names the
    ///   captured position of that field; any other place reaches what its
    ///   local reaches.
    /// - panics: none.
    /// - executable: none — the outcome is a lookup keyed by the place's shape.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture walks captured state inside closures
    ///   built by helpers, by reference and by move.
    /// - witness: `tests::ui_spec_cost`
    fn of_place(
        &self,
        place: Place<'_>,
        closure: ClosureBody,
    ) -> BTreeSet<Position>
    {
        let Place { local, projection } = place;
        if closure == ClosureBody::Closure && local == Local::from_u32(1) {
            let mut projection = projection.iter();
            let field = match projection.next() {
                | Some(ProjectionElem::Deref) => projection.next(),
                | other => other,
            };
            return match field {
                | Some(ProjectionElem::Field(index, _)) => {
                    BTreeSet::from([Position(index.as_u32())])
                },
                | _ => BTreeSet::new(),
            };
        }
        self.0.get(&local).cloned().unwrap_or_default()
    }

    /// The positions an operand reaches.
    ///
    /// # Specification
    /// - ensures: a copied or moved place reaches what [`Reach::of_place`]
    ///   gives; a constant reaches nothing.
    /// - panics: none.
    /// - executable: none — the outcome is [`Reach::of_place`] or empty.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture passes state to helpers by copy, by
    ///   move and by borrow.
    /// - witness: `tests::ui_spec_cost`
    fn of_operand(
        &self,
        operand: &Operand<'_>,
        closure: ClosureBody,
    ) -> BTreeSet<Position>
    {
        match *operand {
            | Operand::Copy(place) | Operand::Move(place) => self.of_place(place, closure),
            | _ => BTreeSet::new(),
        }
    }

    /// Add `positions` to what `local` reaches.
    ///
    /// # Specification
    /// - ensures: `local` reaches a superset of `positions` afterwards; the
    ///   output is [`Grew::Grew`] exactly when it reached fewer before.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the UI fixture reaches walked state through chains of
    ///   borrows and calls inside loops, which settle only by repetition.
    /// - witness: `tests::ui_spec_cost`
    #[spec(ensures: |grew| grew == Grew::Same || self.0.get(&local).is_some_and(|held| !held.is_empty()))]
    fn extend(
        &mut self,
        local: Local,
        positions: BTreeSet<Position>,
    ) -> Grew
    {
        if positions.is_empty() {
            return Grew::Same;
        }
        let held = self.0.entry(local).or_default();
        let before = held.len();
        held.extend(positions);
        if held.len() == before {
            Grew::Same
        }
        else {
            Grew::Grew
        }
    }
}

/// Whether a reach set grew.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Grew
{
    /// The set gained a position.
    Grew,
    /// The set is unchanged.
    Same,
}

/// Whether a body is a closure's, whose environment holds its positions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClosureBody
{
    /// The body's first local is the closure environment.
    Closure,
    /// The body's parameters are its positions.
    Function,
}

/// A closure a body builds and the operands it captures.
struct Built<'tcx>
{
    /// The closure's definition.
    def_id: DefId,
    /// The local the closure is assigned to.
    holder: Local,
    /// The captured operands, in field order.
    operands: Vec<Operand<'tcx>>,
}

/// Read one body against the summaries computed so far.
///
/// # Specification
/// - ensures: [`Reading::Needs`] lists the callees and built closures whose
///   summaries are missing and not themselves being read; otherwise
///   [`Reading::Ready`] holds every position whose reach is walked by a
///   standard-library call, handed to a summarized body that walks it, handed
///   to an unresolved or unreadable callee, or captured by a closure that walks
///   it. Closures handed to the instrumentation, other than the body entry, are
///   the callee's own clauses and are not read.
/// - panics: none.
/// - executable: none — the outcome is a dataflow fixpoint over the body's MIR;
///   a predicate could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture's helpers walk their receiver, a later
///   parameter, a captured value and a value reached through another helper,
///   and carry clauses of their own that are not charged to their callers.
/// - witness: `tests::ui_spec_cost`
fn read_body(
    tcx: TyCtxt<'_>,
    done: &HashMap<DefId, Summary>,
    reading: &HashSet<DefId>,
    def_id: DefId,
) -> Reading
{
    let body = tcx.optimized_mir(def_id);
    let typing_env = TypingEnv::post_analysis(tcx, def_id);
    let closure = if tcx.is_closure_like(def_id) {
        ClosureBody::Closure
    }
    else {
        ClosureBody::Function
    };
    let reach = settle(tcx, body, closure);
    let built = built_closures(body);
    let clauses = clause_closures(tcx, body, &built);
    let mut needs = Vec::new();
    let mut summary = Summary::default();
    let mut charge = |positions: BTreeSet<Position>, witness: Witness| {
        for position in positions {
            summary.record(position, witness);
        }
    };
    let lookup = |target: DefId, needs: &mut Vec<DefId>| {
        done.get(&target).cloned().or_else(|| {
            if reading.contains(&target) {
                Some(Summary::default())
            }
            else {
                needs.push(target);
                None
            }
        })
    };
    for block in body.basic_blocks.iter() {
        let Some(terminator) = block.terminator.as_ref()
        else {
            continue;
        };
        let TerminatorKind::Call {
            ref func,
            ref args,
            destination,
            fn_span,
            ..
        } = terminator.kind
        else {
            continue;
        };
        let Some((callee, callee_args)) = func.const_fn_def()
        else {
            continue;
        };
        let at = |argument: &Spanned<Operand<'_>>| reach.of_operand(&argument.node, closure);
        match classify(tcx, typing_env, callee, callee_args) {
            | Callee::Library(target) => {
                let Some(receiver) = args.first()
                else {
                    continue;
                };
                let receiver_ty = receiver.node.ty(&body.local_decls, tcx);
                let result_ty = destination.ty(&body.local_decls, tcx).ty;
                if let Maybe::Present(walk) = library_walk(
                    tcx,
                    typing_env,
                    tcx.item_name(target),
                    receiver_ty,
                    result_ty,
                ) {
                    // A comparison walks both operands; every other walk, its
                    // receiver.
                    let walked = if walk == Walk::Compares {
                        args.iter().flat_map(at).collect()
                    }
                    else {
                        at(receiver)
                    };
                    charge(walked, Witness {
                        walk,
                        span: fn_span,
                    });
                }
            },
            | Callee::Body(target) => {
                if let Some(found) = lookup(target, &mut needs) {
                    for (position, witness) in found.walks() {
                        if let Maybe::Present(argument) = position.select(args) {
                            charge(at(argument), witness);
                        }
                    }
                }
            },
            | Callee::Unresolved(_) => {
                for argument in &**args {
                    charge(at(argument), Witness {
                        walk: Walk::Unresolved,
                        span: fn_span,
                    });
                }
            },
            | Callee::Unreadable(_) => {
                for argument in &**args {
                    charge(at(argument), Witness {
                        walk: Walk::Unreadable,
                        span: fn_span,
                    });
                }
            },
            | Callee::Instrumentation | Callee::Bounded | Callee::Opaque => {},
        }
    }
    for closure_built in built
        .iter()
        .filter(|candidate| !clauses.contains(&candidate.holder))
    {
        if let Some(found) = lookup(closure_built.def_id, &mut needs) {
            for (position, witness) in found.walks() {
                if let Maybe::Present(operand) = position.select(&closure_built.operands) {
                    charge(reach.of_operand(operand, closure), witness);
                }
            }
        }
    }
    if needs.is_empty() {
        Reading::Ready(summary)
    }
    else {
        Reading::Needs(needs)
    }
}

/// Settle what every local of a body reaches.
///
/// # Specification
/// - ensures: the parameters of a function body reach their own positions; an
///   assignment's destination reaches what its copied, moved, borrowed, cast or
///   aggregated operands reach; a call's destination reaches what its arguments
///   reach when its type holds a borrow. Repeats until nothing grows.
/// - panics: none.
/// - executable: none — the outcome is the fixpoint itself; a predicate could
///   only recompute it.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture reaches walked state through field
///   borrows, dereferences, slice coercions, iterator adapters and loops.
/// - witness: `tests::ui_spec_cost`
fn settle<'tcx>(
    tcx: TyCtxt<'tcx>,
    body: &Body<'tcx>,
    closure: ClosureBody,
) -> Reach
{
    let mut reach = Reach::default();
    if closure == ClosureBody::Function {
        for (index, local) in body.args_iter().enumerate() {
            if let Ok(position) = u32::try_from(index) {
                reach.extend(local, BTreeSet::from([Position(position)]));
            }
        }
    }
    loop {
        let mut grew = Grew::Same;
        for block in body.basic_blocks.iter() {
            for statement in &block.statements {
                let StatementKind::Assign(ref assignment) = statement.kind
                else {
                    continue;
                };
                let (
                    Place {
                        local: assigned, ..
                    },
                    ref rvalue,
                ) = **assignment;
                let positions = match *rvalue {
                    | Rvalue::Use(ref operand, _)
                    | Rvalue::Cast(_, ref operand, _)
                    | Rvalue::Repeat(ref operand, _) => reach.of_operand(operand, closure),
                    | Rvalue::Ref(_, _, borrowed)
                    | Rvalue::RawPtr(_, borrowed)
                    | Rvalue::CopyForDeref(borrowed) => reach.of_place(borrowed, closure),
                    | Rvalue::Aggregate(ref kind, ref operands)
                        if !matches!(**kind, AggregateKind::Closure(..)) =>
                    {
                        operands
                            .iter()
                            .flat_map(|operand| reach.of_operand(operand, closure))
                            .collect()
                    },
                    | _ => BTreeSet::new(),
                };
                if reach.extend(assigned, positions) == Grew::Grew {
                    grew = Grew::Grew;
                }
            }
            let Some(terminator) = block.terminator.as_ref()
            else {
                continue;
            };
            let TerminatorKind::Call {
                ref args,
                destination,
                ..
            } = terminator.kind
            else {
                continue;
            };
            if !destination
                .ty(&body.local_decls, tcx)
                .ty
                .has_erased_regions()
            {
                continue;
            }
            let positions: BTreeSet<Position> = args
                .iter()
                .flat_map(|argument| reach.of_operand(&argument.node, closure))
                .collect();
            let Place {
                local: returned, ..
            } = destination;
            if reach.extend(returned, positions) == Grew::Grew {
                grew = Grew::Grew;
            }
        }
        if grew == Grew::Same {
            return reach;
        }
    }
}

/// The closures a body builds.
///
/// # Specification
/// - ensures: one entry per closure aggregate assigned in the body, with its
///   destination local and captured operands.
/// - panics: none.
/// - executable: none — the outcome is a filter over the body's statements.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture's helpers build closures that capture
///   state by reference and by move.
/// - witness: `tests::ui_spec_cost`
fn built_closures<'tcx>(body: &Body<'tcx>) -> Vec<Built<'tcx>>
{
    body.basic_blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| {
            let StatementKind::Assign(ref assignment) = statement.kind
            else {
                return None;
            };
            let (Place { local: holder, .. }, ref rvalue) = **assignment;
            let Rvalue::Aggregate(ref kind, ref operands) = *rvalue
            else {
                return None;
            };
            let AggregateKind::Closure(def_id, _) = **kind
            else {
                return None;
            };
            Some(Built {
                def_id,
                holder,
                operands: operands.iter().cloned().collect(),
            })
        })
        .collect()
}

/// The locals holding closures that the instrumentation evaluates as clauses.
///
/// # Specification
/// - ensures: a built closure's local is included exactly when it is an
///   argument of a call into the instrumentation crate other than the body
///   entry.
/// - panics: none.
/// - executable: none — the outcome is a filter over the body's calls.
///
/// # Adequacy
/// - hypothesis: L3 — the UI fixture's helpers carry clauses that walk their
///   own state, which their callers' clauses are not charged for.
/// - witness: `tests::ui_spec_cost`
fn clause_closures(
    tcx: TyCtxt<'_>,
    body: &Body<'_>,
    built: &[Built<'_>],
) -> HashSet<Local>
{
    let built: HashSet<Local> = built.iter().map(|closure| closure.holder).collect();
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match terminator.kind {
            | TerminatorKind::Call {
                ref func, ref args, ..
            } => Some((func, args)),
            | _ => None,
        })
        .filter(|&(func, _)| {
            func.const_fn_def().is_some_and(|(callee, _)| {
                tcx.crate_name(callee.krate).as_str() == INSTRUMENTATION_CRATE
                    && tcx.item_name(callee).as_str() != BODY_ENTRY
            })
        })
        .flat_map(|(_, args)| args.iter())
        .filter_map(|argument| match argument.node {
            | Operand::Copy(Place { local, .. }) | Operand::Move(Place { local, .. }) => {
                Some(local)
            },
            | _ => None,
        })
        .filter(|local| built.contains(local))
        .collect()
}
