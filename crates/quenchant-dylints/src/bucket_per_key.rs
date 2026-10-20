//! A map that allocates a growable bucket for every key it records.
//!
//! `entry(key).or_default().push(value)` on a map whose value is a `Vec`,
//! `VecDeque`, `String` or a `SmallVec` with no inline capacity allocates the
//! bucket's buffer the first time a key is seen. When keys are near-injective —
//! a digest of the value, an identity — almost every bucket holds one entry,
//! and every recorded value pays a heap allocation besides its share of the
//! map's own growth: `n` entries cost `n` allocations where an inline first
//! entry, or one contiguous vector with a chain head per key, costs none in
//! steady state.
//!
//! The reads tell a collision chain from a group. A map whose buckets are
//! only ever searched for one entry — `get(key)` followed by `iter().find(..)`,
//! `position`, `any`, `contains` — holds a bucket because two keys may
//! collide, not because a key gathers many values; that map is refused. A map
//! one of whose reads takes a bucket whole — iterated, counted, handed on —
//! is a grouping, and its buckets genuinely hold several values.
//!
//! # Boundaries
//!
//! The map is followed across the whole crate: a local through its body, a
//! struct field through every body that projects it. A field reachable from
//! another crate, a map behind a reference, and a map matched by a struct
//! pattern can be read where this pass does not look, and are accepted. A
//! bucket filled by `extend`, `append`, `insert` of a whole value or through a
//! bound entry is filled in bulk and accepted. Value types with inline-first
//! storage are not growable buckets. Expressions from macro expansions and
//! inside `spec` attributes are neither insertions nor reads.
//!
//! No Clippy lint prices a bucket per key.

use core::ops::ControlFlow;
use std::collections::HashMap;

use clippy_utils::diagnostics::span_lint_hir_and_then;
use clippy_utils::res::MaybeResPath as _;
use clippy_utils::visitors::for_each_expr;
use quenchant_shape::shape::Maybe;
use rustc_hir::Body;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::LangItem;
use rustc_hir::MatchSource;
use rustc_hir::Node;
use rustc_hir::Pat;
use rustc_hir::PatKind;
use rustc_hir::QPath;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_middle::ty::Ty;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;

use crate::clause_cost::ClauseRegions;
use crate::cost_shape::Inspection;
use crate::cost_shape::inspection;
use crate::walk_summary::standard_name;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a single-element insertion — `push`, `push_back`, `push_front`, `push_str` or `insert` — into a bucket reached through `entry(key).or_default()`, `or_insert_with(Vec::new)` or `or_insert(Vec::new())` of a `HashMap`, `BTreeMap`, `IndexMap` or hashbrown `HashMap` whose value is a `Vec`, `VecDeque`, `String` or a `SmallVec` with no inline capacity, when every read of the map's buckets in the crate searches a bucket for one entry and no insertion fills a bucket in bulk.
    ///
    /// ### Why is this bad?
    ///
    /// The first insertion under a key allocates the bucket's buffer. A map whose buckets are only searched for one entry is a collision chain: its keys are near-injective, almost every bucket holds one entry, and every recorded entry pays a heap allocation. An inline first entry (`SmallVec<[_; 1]>`, or an enum of one entry and a spilled vector) or one contiguous vector of entries with a chain head per key allocates nothing per entry in steady state.
    ///
    /// ### Accepted forms
    ///
    /// The value's type decides: a bucket with inline-first storage is accepted. The reads decide too: a map one of whose reads takes a bucket whole — iterates it, counts it, hands it on, iterates the map — groups values and is accepted, and so is a map filled by `extend`, `append` or an `insert` of a whole value. A field visible outside the crate, a map behind a reference and a map destructured by a pattern are accepted, their reads being out of sight.
    ///
    /// ### Clippy
    ///
    /// No Clippy lint prices a bucket per key.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// struct Memo { buckets: BTreeMap<Digest, Vec<Entry>> }
    /// impl Memo {
    ///     fn recall(&self, key: &Key) -> Option<&Entry> {
    ///         self.buckets.get(&key.digest())?.iter().find(|entry| entry.key == *key)
    ///     }
    ///     fn remember(&mut self, entry: Entry) {
    ///         self.buckets.entry(entry.key.digest()).or_default().push(entry);
    ///     }
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// struct Memo { buckets: BTreeMap<Digest, SmallVec<[Entry; 1]>> }
    /// ```
    pub BUCKET_PER_KEY,
    Deny,
    "a map allocates a growable bucket for every key, and its buckets are only searched for one entry"
}

quenchant_shape::reason_enum! {
    /// A map's buckets can cost no allocation per key, or can be needed as
    /// groups.
    pub mod bucket_reading {
        /// Why the map's insertions are not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrefused {
            /// The value is a `SmallVec` whose inline capacity holds an entry
            /// before any allocation.
            InlineFirst,
            /// The map is a field visible outside the crate, is reached
            /// through a reference, or is destructured by a pattern: some of
            /// its reads are out of sight.
            OutOfSight,
            /// Some insertion fills a bucket in bulk, or through an entry this
            /// pass does not follow.
            FilledInBulk,
            /// Some read takes a bucket whole, or the map whole.
            ReadAsGroups,
        }
    }
}

/// A map followed across the crate: a local binding or a struct field.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum MapId
{
    /// A local binding that holds the map by value.
    Local(HirId),
    /// A struct field that holds the map by value.
    Field(DefId),
}

/// One single-element insertion into a bucket.
#[derive(Clone, Copy, Debug)]
struct Push
{
    /// The insertion call.
    call: HirId,
    /// From the insertion's method name to the call's end.
    at: Span,
    /// The map the bucket was reached through.
    map: Span,
}

/// What the crate does with one map.
#[derive(Debug)]
struct Record
{
    /// The single-element insertions through a defaulted entry.
    pushes: Vec<Push>,
    /// The first reason found to accept every insertion, if any.
    accepted: Maybe<bucket_reading::Unrefused, Refusable>,
}

/// No reason to accept a map's insertions has been found yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Refusable
{
    /// Every use so far is a bucket insertion, a search or neutral.
    SoFar,
}

impl Record
{
    /// A map with no use recorded yet.
    ///
    /// # Specification
    /// trivial.
    const fn new() -> Self
    {
        Self {
            pushes: Vec::new(),
            accepted: Maybe::Absent(Refusable::SoFar),
        }
    }

    /// Keep the first reason found to accept the map.
    ///
    /// # Specification
    /// - ensures: `accepted` is `reason` when it held no reason, and is
    ///   unchanged otherwise.
    /// - panics: none.
    /// - executable: none — the record is pass state; the clause would repeat
    ///   the one comparison the body makes.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the accepted fixture adds each reason to a map that
    ///   would otherwise be refused.
    /// - witness: `tests::ui_cost_shape`
    fn accept(
        &mut self,
        reason: bucket_reading::Unrefused,
    )
    {
        if matches!(self.accepted, Maybe::Absent(Refusable::SoFar)) {
            self.accepted = Maybe::Present(reason);
        }
    }
}

/// Bucket-per-key policy; specification clauses are left to
/// `spec_clause_traversal`.
pub struct BucketPerKey
{
    /// The authored `spec` attribute ranges, whose clauses are not inspected.
    regions: ClauseRegions,
    /// Every candidate map used in the crate, with what its uses decide.
    maps: HashMap<MapId, Record>,
}

impl BucketPerKey
{
    /// The pass reads the ranges the pre-expansion clause collector records.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub fn new(regions: ClauseRegions) -> Self
    {
        Self {
            regions,
            maps: HashMap::new(),
        }
    }
}

impl_lint_pass!(BucketPerKey => [BUCKET_PER_KEY]);

impl<'tcx> LateLintPass<'tcx> for BucketPerKey
{
    /// Record what each use of a candidate map does.
    ///
    /// # Specification
    /// - ensures: every inspected expression that names a candidate map
    ///   ([`candidate`]) adds a single-element insertion to the map's record or
    ///   the reason [`classify_use`] gives to accept it; a map's reason is the
    ///   first one found.
    /// - ensures: expressions from macro expansions and inside `spec`
    ///   attributes are not inspected ([`inspection`]).
    /// - panics: none.
    /// - executable: none — the record is pass state read only at the crate's
    ///   end.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse a field and a local map and
    ///   accept each reason beside them.
    /// - witness: `tests::ui_cost_shape`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        if inspection(&self.regions, expr) == Inspection::Skipped {
            return;
        }
        let map = match candidate(cx, expr) {
            Maybe::Present(map) => map,
            Maybe::Absent(Candidacy::NotAMap) => return,
            Maybe::Absent(Candidacy::Unrefused(id, reason)) => {
                self.maps
                    .entry(id)
                    .or_insert_with(Record::new)
                    .accept(reason);
                return;
            },
        };
        let record = self.maps.entry(map).or_insert_with(Record::new);
        match classify_use(cx, expr) {
            Use::Push(push) => record.pushes.push(push),
            Use::Neutral => {},
            Use::Accepts(reason) => record.accept(reason),
        }
    }

    /// Accept a field map destructured by a struct pattern.
    ///
    /// # Specification
    /// - ensures: a struct pattern that binds a field holding a growable bucket
    ///   map records [`bucket_reading::Unrefused::OutOfSight`] for that field.
    /// - panics: none.
    /// - executable: none — the record is pass state read only at the crate's
    ///   end.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the accepted fixture destructures a struct whose map
    ///   would otherwise be refused.
    /// - witness: `tests::ui_cost_shape`
    fn check_pat(
        &mut self,
        cx: &LateContext<'tcx>,
        pat: &'tcx Pat<'tcx>,
    )
    {
        let PatKind::Struct(_, fields, _) = pat.kind
        else {
            return;
        };
        let ty::Adt(definition, args) = *cx.typeck_results().pat_ty(pat).peel_refs().kind()
        else {
            return;
        };
        if !definition.is_struct() {
            return;
        }
        let variant = definition.non_enum_variant();
        for field in fields {
            let Some(declared) = cx
                .typeck_results()
                .opt_field_index(field.hir_id)
                .and_then(|index| variant.fields.get(index))
            else {
                continue;
            };
            let declared_type = declared.ty(cx.tcx, args).skip_norm_wip();
            if !matches!(bucket_value(cx, declared_type), Value::NotABucket) {
                self.maps
                    .entry(MapId::Field(declared.did))
                    .or_insert_with(Record::new)
                    .accept(bucket_reading::Unrefused::OutOfSight);
            }
        }
    }

    /// Report every insertion into a map no use accepts.
    ///
    /// # Specification
    /// - ensures: reports each recorded insertion of each map whose record
    ///   holds no reason to accept it, in source order, at the insertion's
    ///   expression so that an `expect` on an enclosing item applies.
    /// - panics: none.
    /// - executable: none — rustc emits diagnostics without a queryable
    ///   per-call diagnostic result.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse the memo shape on a field and a
    ///   local and accept a grouped map, a bulk-filled map and a set bucket.
    /// - witness: `tests::ui_cost_shape`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        let mut refused: Vec<Push> = self
            .maps
            .values()
            .filter(|record| matches!(record.accepted, Maybe::Absent(Refusable::SoFar)))
            .flat_map(|record| record.pushes.iter().copied())
            .collect();
        refused.sort_by_key(|push| push.at.lo());
        for push in refused {
            span_lint_hir_and_then(
                cx,
                BUCKET_PER_KEY,
                push.call,
                push.at,
                "this allocates a bucket for each key of a map whose buckets are only searched for \
                 one entry",
                |diagnostic| {
                    diagnostic.span_label(push.map, "every read of this map searches one bucket");
                    diagnostic.help(
                        "keep a bucket's first entry inline and spill only collisions \
                         (`SmallVec<[_; 1]>`, or an enum of one entry and a vector), or hold the \
                         entries in one vector and map each key to its chain's head",
                    );
                },
            );
        }
    }
}

/// Whether an expression names a candidate map, and which.
#[derive(Clone, Copy, Debug)]
enum Candidacy
{
    /// The expression names no map with a growable bucket.
    NotAMap,
    /// The expression names a bucket map whose insertions are accepted.
    Unrefused(MapId, bucket_reading::Unrefused),
}

/// The map an expression names, when its value is a growable bucket.
///
/// # Specification
/// - ensures: `Present(Local)` when `expr` is a path to a local whose own type
///   is a map with a growable bucket ([`bucket_value`]); `Present(Field)` when
///   `expr` projects a struct field whose declared type is one and which is not
///   exported from the crate.
/// - ensures: `Absent(Unrefused(_, OutOfSight))` for an exported field and for
///   a local holding such a map behind a reference; `Absent(Unrefused(_,
///   InlineFirst))` for an inline-first bucket; `Absent(NotAMap)` otherwise.
/// - panics: none.
/// - executable: none — the outcome is read from the typeck results, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a field and a local and accept an
///   exported field, a map passed by reference and a destructured struct; the
///   `SmallVec` arm has no fixture, the UI suite building without `smallvec`.
/// - witness: `tests::ui_cost_shape`
fn candidate<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Maybe<MapId, Candidacy>
{
    let typeck = cx.typeck_results();
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a path \
                  or a field projection names a map"
    )]
    let id = match expr.kind {
        ExprKind::Path(QPath::Resolved(None, path)) => match path.res {
            Res::Local(local) => MapId::Local(local),
            _ => return Maybe::Absent(Candidacy::NotAMap),
        },
        ExprKind::Field(base, _) => {
            let Some(index) = typeck.opt_field_index(expr.hir_id)
            else {
                return Maybe::Absent(Candidacy::NotAMap);
            };
            let ty::Adt(definition, _) = *typeck.expr_ty_adjusted(base).peel_refs().kind()
            else {
                return Maybe::Absent(Candidacy::NotAMap);
            };
            if !definition.is_struct() {
                return Maybe::Absent(Candidacy::NotAMap);
            }
            let Some(field) = definition.non_enum_variant().fields.get(index)
            else {
                return Maybe::Absent(Candidacy::NotAMap);
            };
            MapId::Field(field.did)
        },
        _ => return Maybe::Absent(Candidacy::NotAMap),
    };
    let held = typeck.expr_ty(expr);
    let value = bucket_value(cx, held.peel_refs());
    match value {
        Value::NotABucket => return Maybe::Absent(Candidacy::NotAMap),
        Value::InlineFirst => {
            return Maybe::Absent(Candidacy::Unrefused(
                id,
                bucket_reading::Unrefused::InlineFirst,
            ));
        },
        Value::Growable => {},
    }
    let out_of_sight = match id {
        MapId::Local(_) => held.is_ref(),
        MapId::Field(field) => field
            .as_local()
            .is_none_or(|local| cx.effective_visibilities.is_exported(local)),
    };
    if out_of_sight {
        Maybe::Absent(Candidacy::Unrefused(
            id,
            bucket_reading::Unrefused::OutOfSight,
        ))
    }
    else {
        Maybe::Present(id)
    }
}

/// What a map's value type is as a bucket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Value
{
    /// The type is not a map, or its value is not a growable collection.
    NotABucket,
    /// The value is a `SmallVec` with inline capacity.
    InlineFirst,
    /// The value allocates on its first insertion.
    Growable,
}

/// Classify a map type by its value type.
///
/// # Specification
/// - ensures: `Growable` when `held` is a standard `HashMap` or `BTreeMap`, an
///   `indexmap` `IndexMap` or a `hashbrown` `HashMap` whose value is a `Vec`,
///   `VecDeque`, `String` or a `SmallVec` with inline capacity zero;
///   `InlineFirst` for a `SmallVec` value with a nonzero or unknown inline
///   capacity; `NotABucket` otherwise.
/// - panics: none.
/// - executable: none — the outcome is read from the type's definition and
///   arguments, which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `Vec`, `VecDeque` and `String`
///   buckets and accept a set bucket; the `SmallVec` arms have no fixture, the
///   UI suite building without `smallvec`.
/// - witness: `tests::ui_cost_shape`
fn bucket_value<'tcx>(
    cx: &LateContext<'tcx>,
    held: Ty<'tcx>,
) -> Value
{
    let ty::Adt(map, args) = *held.kind()
    else {
        return Value::NotABucket;
    };
    let map_name = cx.tcx.get_diagnostic_name(map.did());
    let foreign = (
        cx.tcx.crate_name(map.did().krate),
        cx.tcx.item_name(map.did()),
    );
    let is_map = matches!(
        map_name.as_ref().map(Symbol::as_str),
        Some("HashMap" | "BTreeMap")
    ) || matches!(
        (foreign.0.as_str(), foreign.1.as_str()),
        ("indexmap", "IndexMap") | ("hashbrown", "HashMap")
    );
    if !is_map || args.len() < 2 {
        return Value::NotABucket;
    }
    let ty::Adt(bucket, bucket_args) = *args.type_at(1).kind()
    else {
        return Value::NotABucket;
    };
    if matches!(
        standard_name(cx.tcx, bucket.did()),
        Maybe::Present(name) if matches!(name.as_str(), "Vec" | "VecDeque" | "String")
    ) {
        return Value::Growable;
    }
    if cx.tcx.crate_name(bucket.did().krate).as_str() != "smallvec"
        || cx.tcx.item_name(bucket.did()).as_str() != "SmallVec"
    {
        return Value::NotABucket;
    }
    let inline = bucket_args.iter().find_map(|argument| {
        if let Some(constant) = argument.as_const() {
            return constant.try_to_target_usize(cx.tcx);
        }
        let ty::Array(_, length) = *argument.as_type()?.kind()
        else {
            return None;
        };
        length.try_to_target_usize(cx.tcx)
    });
    if inline == Some(0) {
        Value::Growable
    }
    else {
        Value::InlineFirst
    }
}

/// What one use of a candidate map does.
#[derive(Clone, Copy, Debug)]
enum Use
{
    /// A single-element insertion into a defaulted bucket.
    Push(Push),
    /// A use that neither inserts nor reads a bucket: a length, a key test, a
    /// key walk, a clear, an assignment of the whole map, a search.
    Neutral,
    /// A use that makes the map's insertions acceptable.
    Accepts(bucket_reading::Unrefused),
}

/// Classify one use of a candidate map by what encloses it.
///
/// # Specification
/// - ensures: `Push` when `map` is the receiver of `entry(_)` followed by a
///   defaulting call ([`defaulted`]) and a `push`, `push_back`, `push_front`,
///   `push_str` or `insert`; `Accepts(FilledInBulk)` when the entry is used in
///   any other way, and for `insert`, `extend` and `append` on the map.
/// - ensures: `Neutral` for `len`, `is_empty`, `contains_key`, `keys`,
///   `into_keys`, `clear`, `capacity`, `reserve` and `shrink_to_fit`, for a
///   `remove` whose result is discarded, for the left side of an assignment,
///   and for `get` or `get_mut` whose bucket is only searched
///   ([`option_searched`]).
/// - ensures: `Accepts(ReadAsGroups)` for every other use.
/// - panics: none.
/// - executable: none — the outcome is decided by walking the parents of the
///   use; a predicate could only repeat that walk.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse a push through `or_default`,
///   `or_insert_with(Vec::new)` and `or_insert(Vec::new())` beside searches
///   through `?`, `if let` and a closure, and accept iteration of the map, a
///   bucket's length, a bucket handed on, `extend` on an entry and `insert` of
///   a whole bucket.
/// - witness: `tests::ui_cost_shape`
fn classify_use<'tcx>(
    cx: &LateContext<'tcx>,
    map: &'tcx Expr<'tcx>,
) -> Use
{
    let Node::Expr(parent) = cx.tcx.parent_hir_node(map.hir_id)
    else {
        return Use::Accepts(bucket_reading::Unrefused::ReadAsGroups);
    };
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`ExprKind` is rustc's, its variants moving with the pinned nightly; only a method \
                  call on the map or an assignment to it is classified"
    )]
    match parent.kind {
        ExprKind::Assign(target, ..) if target.hir_id == map.hir_id => Use::Neutral,
        ExprKind::MethodCall(segment, receiver, ..) if receiver.hir_id == map.hir_id => {
            match segment.ident.name.as_str() {
                "entry" => entry_use(cx, map, parent),
                "get" | "get_mut" => match option_searched(cx, parent) {
                    Searched::Only => Use::Neutral,
                    Searched::Whole => Use::Accepts(bucket_reading::Unrefused::ReadAsGroups),
                },
                "len" | "is_empty" | "contains_key" | "keys" | "into_keys" | "clear"
                | "capacity" | "reserve" | "shrink_to_fit" => Use::Neutral,
                "remove" if matches!(cx.tcx.parent_hir_node(parent.hir_id), Node::Stmt(_)) => {
                    Use::Neutral
                },
                "insert" | "extend" | "append" => {
                    Use::Accepts(bucket_reading::Unrefused::FilledInBulk)
                },
                _ => Use::Accepts(bucket_reading::Unrefused::ReadAsGroups),
            }
        },
        _ => Use::Accepts(bucket_reading::Unrefused::ReadAsGroups),
    }
}

/// Classify an `entry(_)` call on a candidate map by what follows it.
///
/// # Specification
/// - requires: `call` is a method call `entry(_)` whose receiver is `map`.
/// - ensures: `Push` when `call` is the receiver of a defaulting call
///   ([`defaulted`]) that is itself the receiver of `push`, `push_back`,
///   `push_front`, `push_str` or `insert`; `Accepts(FilledInBulk)` otherwise.
/// - panics: none.
/// - executable: none — the outcome is decided by walking the parents of the
///   call; a predicate could only repeat that walk.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse each defaulting spelling followed by
///   `push`, and accept `extend` and an entry bound to a local.
/// - witness: `tests::ui_cost_shape`
fn entry_use<'tcx>(
    cx: &LateContext<'tcx>,
    map: &'tcx Expr<'tcx>,
    call: &'tcx Expr<'tcx>,
) -> Use
{
    let bulk = Use::Accepts(bucket_reading::Unrefused::FilledInBulk);
    let Node::Expr(default) = cx.tcx.parent_hir_node(call.hir_id)
    else {
        return bulk;
    };
    if !matches!(defaulted(cx, call, default), Defaulted::Empty) {
        return bulk;
    }
    let Node::Expr(insertion) = cx.tcx.parent_hir_node(default.hir_id)
    else {
        return bulk;
    };
    let ExprKind::MethodCall(segment, receiver, ..) = insertion.kind
    else {
        return bulk;
    };
    let single = matches!(
        segment.ident.name.as_str(),
        "push" | "push_back" | "push_front" | "push_str" | "insert"
    );
    if receiver.hir_id != default.hir_id || !single {
        return bulk;
    }
    Use::Push(Push {
        call: insertion.hir_id,
        at: segment.ident.span.with_hi(insertion.span.hi()),
        map: map.span,
    })
}

/// Whether a call on an entry yields its bucket empty when the key is new.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Defaulted
{
    /// The call inserts an empty bucket for a new key.
    Empty,
    /// The call is anything else.
    Other,
}

/// Classify the call that follows `entry(_)`.
///
/// # Specification
/// - ensures: `Empty` when `call` is the receiver of `default`, and `default`
///   is `or_default()`, `or_insert_with(f)` where `f` names a `new` or
///   `default` function or is a closure whose body calls one with no arguments,
///   or `or_insert(e)` where `e` is such a call; `Other` otherwise.
/// - panics: none.
/// - executable: none — the outcome is decided by reading the call's arguments;
///   a predicate could only repeat that reading.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures refuse `or_default()`,
///   `or_insert_with(Vec::new)`, `or_insert_with(|| Vec::new())` and
///   `or_insert(Vec::new())`, and accept `or_insert(vec![first])`.
/// - witness: `tests::ui_cost_shape`
fn defaulted<'tcx>(
    cx: &LateContext<'tcx>,
    call: &'tcx Expr<'tcx>,
    default: &'tcx Expr<'tcx>,
) -> Defaulted
{
    let empty_constructor = |callee: &Expr<'_>| {
        let ExprKind::Path(ref path) = callee.kind
        else {
            return false;
        };
        if let Res::Def(DefKind::Fn | DefKind::AssocFn, function) =
            cx.qpath_res(path, callee.hir_id)
        {
            matches!(cx.tcx.item_name(function).as_str(), "new" | "default")
        }
        else {
            false
        }
    };
    let empty_call = |value: &Expr<'_>| matches!(value.kind, ExprKind::Call(callee, []) if empty_constructor(callee));
    let ExprKind::MethodCall(segment, receiver, arguments, _) = default.kind
    else {
        return Defaulted::Other;
    };
    if receiver.hir_id != call.hir_id {
        return Defaulted::Other;
    }
    let empty = match (segment.ident.name.as_str(), arguments) {
        ("or_default", &[]) => true,
        ("or_insert", &[ref value]) => empty_call(value),
        ("or_insert_with", &[ref function]) => {
            if let ExprKind::Closure(closure) = function.kind {
                empty_call(cx.tcx.hir_body(closure.body).value)
            }
            else {
                empty_constructor(function)
            }
        },
        _ => false,
    };
    if empty {
        Defaulted::Empty
    }
    else {
        Defaulted::Other
    }
}

/// Whether a bucket read reaches past one searched entry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Searched
{
    /// Every use of the bucket searches it for one entry.
    Only,
    /// Some use takes the bucket whole.
    Whole,
}

/// Follow the optional bucket `get(_)` or `get_mut(_)` returns.
///
/// # Specification
/// - ensures: `Only` when `lookup`'s result is unwrapped by `?` into a bucket
///   only searched, bound by an `if let`, `let`-`else` or `let` pattern
///   `Some(bucket)` whose every use is a search, handed to a closure of `map`,
///   `and_then`, `is_some_and` or `is_none_or` whose parameter is only
///   searched, or tested by `is_some` or `is_none`; `Whole` otherwise.
/// - panics: none.
/// - executable: none — the outcome is decided by walking the parents of the
///   lookup and the uses of its bindings; a predicate could only repeat that
///   walk.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures search through `?`, `if let`, a closure of
///   `and_then` and an `is_some` test, and accept a bucket whose length is read
///   and one handed to a function.
/// - witness: `tests::ui_cost_shape`
fn option_searched<'tcx>(
    cx: &LateContext<'tcx>,
    lookup: &'tcx Expr<'tcx>,
) -> Searched
{
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`Node` and `ExprKind` are rustc's, their variants moving with the pinned \
                  nightly; only a `?`, a pattern binding or an option method carries the bucket"
    )]
    match cx.tcx.parent_hir_node(lookup.hir_id) {
        Node::Expr(branch) => match branch.kind {
            ExprKind::Call(callee, &[_])
                if is_lang_path(cx, callee, LangItem::TryTraitBranch) == LangPath::Is =>
            {
                match cx.tcx.parent_hir_node(branch.hir_id) {
                    Node::Expr(unwrapped)
                        if matches!(
                            unwrapped.kind,
                            ExprKind::Match(_, _, MatchSource::TryDesugar(_))
                        ) =>
                    {
                        value_searched(cx, unwrapped)
                    },
                    _ => Searched::Whole,
                }
            },
            ExprKind::Let(binding) => some_binding_searched(cx, binding.pat),
            ExprKind::MethodCall(segment, receiver, arguments, _)
                if receiver.hir_id == lookup.hir_id =>
            {
                match (segment.ident.name.as_str(), arguments) {
                    ("is_some" | "is_none", &[]) => Searched::Only,
                    ("map" | "and_then" | "is_some_and" | "is_none_or", &[ref closure]) => {
                        let ExprKind::Closure(closure) = closure.kind
                        else {
                            return Searched::Whole;
                        };
                        let body = cx.tcx.hir_body(closure.body);
                        match *body.params {
                            [ref parameter] => binding_searched(cx, parameter.pat, body),
                            _ => Searched::Whole,
                        }
                    },
                    _ => Searched::Whole,
                }
            },
            _ => Searched::Whole,
        },
        Node::LetStmt(binding) => some_binding_searched(cx, binding.pat),
        _ => Searched::Whole,
    }
}

/// Whether a path names a language item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LangPath
{
    /// The path is the language item.
    Is,
    /// It is not.
    IsNot,
}

/// Decide whether `callee` is a path to the language item `item`.
///
/// # Specification
/// - ensures: answers whether `callee` is a lang-item path to `item`, or a
///   resolved path to the item's definition.
/// - panics: none.
/// - executable: none — the outcome is the resolution itself.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures unwrap a bucket with `?`, whose desugaring
///   calls the `branch` language item.
/// - witness: `tests::ui_cost_shape`
fn is_lang_path(
    cx: &LateContext<'_>,
    callee: &Expr<'_>,
    item: LangItem,
) -> LangPath
{
    let ExprKind::Path(ref path) = callee.kind
    else {
        return LangPath::IsNot;
    };
    let target = cx.tcx.lang_items().get(item);
    let matches = if let Res::Def(_, definition) = cx.qpath_res(path, callee.hir_id) {
        target == Some(definition)
    }
    else {
        false
    };
    if matches {
        LangPath::Is
    }
    else {
        LangPath::IsNot
    }
}

/// Follow a `Some(bucket)` pattern's binding.
///
/// # Specification
/// - ensures: `Only` when `pattern` is `Some(p)` with `p` a plain binding whose
///   every use in the enclosing body is a search ([`bucket_searched`]); `Whole`
///   otherwise.
/// - panics: none.
/// - executable: none — the outcome is decided by the use walk; a predicate
///   could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures search an `if let Some(bucket)` binding and
///   accept one whose length is read.
/// - witness: `tests::ui_cost_shape`
fn some_binding_searched<'tcx>(
    cx: &LateContext<'tcx>,
    pattern: &'tcx Pat<'tcx>,
) -> Searched
{
    let PatKind::TupleStruct(_, &[ref inner], _) = pattern.kind
    else {
        return Searched::Whole;
    };
    let Some(body) = cx.enclosing_body
    else {
        return Searched::Whole;
    };
    binding_searched(cx, inner, cx.tcx.hir_body(body))
}

/// Follow a plain binding's uses in a body.
///
/// # Specification
/// - ensures: `Only` when `pattern` is a plain binding and every path in `body`
///   that names it is a search ([`bucket_searched`]); `Whole` otherwise.
/// - panics: none.
/// - executable: none — the outcome is decided by the use walk; a predicate
///   could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures search a bound bucket and a closure
///   parameter, and accept a bound bucket handed on.
/// - witness: `tests::ui_cost_shape`
fn binding_searched<'tcx>(
    cx: &LateContext<'tcx>,
    pattern: &'tcx Pat<'tcx>,
    body: &'tcx Body<'tcx>,
) -> Searched
{
    let PatKind::Binding(_, local, _, None) = pattern.kind
    else {
        return Searched::Whole;
    };
    let whole = for_each_expr(cx.tcx, body.value, |inner| {
        if inner.res_local_id() == Some(local) && bucket_searched(cx, inner) == Searched::Whole {
            ControlFlow::Break(())
        }
        else {
            ControlFlow::Continue(())
        }
    });
    if whole.is_some() {
        Searched::Whole
    }
    else {
        Searched::Only
    }
}

/// Follow an unwrapped bucket to its uses.
///
/// # Specification
/// - ensures: when `bucket` initializes a `let` statement, the result of
///   [`binding_searched`] over the enclosing body for its pattern; otherwise
///   the result of [`bucket_searched`] for `bucket` itself.
/// - panics: none.
/// - executable: none — the outcome is decided by the use walk; a predicate
///   could only repeat it.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures bind a bucket unwrapped by `?` and search
///   it, and search one unwrapped in place.
/// - witness: `tests::ui_cost_shape`
fn value_searched<'tcx>(
    cx: &LateContext<'tcx>,
    bucket: &'tcx Expr<'tcx>,
) -> Searched
{
    match (cx.tcx.parent_hir_node(bucket.hir_id), cx.enclosing_body) {
        (Node::LetStmt(binding), Some(body)) => {
            binding_searched(cx, binding.pat, cx.tcx.hir_body(body))
        },
        _ => bucket_searched(cx, bucket),
    }
}

/// Classify one use of a bucket.
///
/// # Specification
/// - ensures: `Only` when `bucket` is the receiver of `contains` or a
///   `binary_search` method, or of `iter()` or `iter_mut()` whose result is the
///   receiver of `find`, `find_map`, `position`, `rposition`, `any` or `all`;
///   `Whole` otherwise.
/// - panics: none.
/// - executable: none — the outcome is decided by reading two parents; a
///   predicate could only read them again.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures search with `iter().find`, `iter_mut().find`
///   and `contains`, and accept `len()` and a `for` loop.
/// - witness: `tests::ui_cost_shape`
fn bucket_searched(
    cx: &LateContext<'_>,
    bucket: &Expr<'_>,
) -> Searched
{
    let Node::Expr(call) = cx.tcx.parent_hir_node(bucket.hir_id)
    else {
        return Searched::Whole;
    };
    let ExprKind::MethodCall(segment, receiver, ..) = call.kind
    else {
        return Searched::Whole;
    };
    if receiver.hir_id != bucket.hir_id {
        return Searched::Whole;
    }
    match segment.ident.name.as_str() {
        "contains" | "binary_search" | "binary_search_by" | "binary_search_by_key" => {
            Searched::Only
        },
        "iter" | "iter_mut" => {
            let Node::Expr(search) = cx.tcx.parent_hir_node(call.hir_id)
            else {
                return Searched::Whole;
            };
            if let ExprKind::MethodCall(segment, receiver, ..) = search.kind
                && receiver.hir_id == call.hir_id
                && matches!(
                    segment.ident.name.as_str(),
                    "find" | "find_map" | "position" | "rposition" | "any" | "all"
                )
            {
                Searched::Only
            }
            else {
                Searched::Whole
            }
        },
        _ => Searched::Whole,
    }
}
