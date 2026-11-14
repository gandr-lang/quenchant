//! Dependency records: the items each executable clause names, and the items
//! each item names.
//!
//! When an item changes, every specification that names it owes
//! re-verification. A specification names an item in two ways: an executable
//! clause of its `#[spec]` attribute calls, reads or matches it, or a prose
//! clause links it. This pass records the first kind and indexes every item a
//! record can name; `quenchant-gates` reads the second kind from rustdoc's JSON
//! output and joins both on this index.
//!
//! A reader of a workspace also asks the converse question: which items does
//! no other item name? The pass records the names of every item's body and
//! signature too. With them, a reader finds an item that only tests name, and
//! an exported item that no other crate names.
//!
//! Clauses are located by source, as [`crate::clause_cost`] locates them: a
//! pre-expansion pass records each clause's key and the source range of its
//! value, and the late pass reads the expressions and patterns whose call site
//! those ranges enclose, which is where the attribute's predicates land after
//! expansion. The expression's HIR owner is the specified item. The same
//! pre-expansion pass records where each item's outer attributes begin: after
//! expansion an attribute macro is gone and a parsed attribute carries no
//! general span, so the authored attributes are only visible there.
//!
//! # Boundaries
//!
//! **Names are resolved, not read.** A clause's names are the definitions the
//! compiler resolved for the typed expressions and patterns inside the clause's
//! source range: a path to a function, constant, static, type, trait or
//! variant; a struct literal or pattern; a method call, resolved to its
//! implementation where the types determine one; a field read. A local, a
//! parameter and a closure are not dependencies, and neither is anything in the
//! standard library or the specification instrumentation. A call is one edge to
//! its callee: the callee's body is not followed, so an item reached only
//! through calls is not named. Following the call graph would flag nearly every
//! specification on a change to the core; the named set is what the clause
//! states a claim about.
//!
//! **An item names what its clauses would name, wherever the name stands.**
//! The pass reads every expression and pattern of an item's body as it reads a
//! clause's. It also reads the paths of the item's signature, of its bounds
//! and of an implementation's header. A closure's names belong to the item that
//! holds the closure. An implementation block holds the names of its header.
//! An import and a re-export name nothing. An item that names itself, as a
//! recursive function does, gets no name of its own. An inherent
//! implementation block prints as its type's path, so the type in its header
//! is no name of the block either.
//!
//! # Items
//!
//! **Records, not diagnostics.** The pass runs only when the Dylint
//! configuration names an `evidence_directory`, and then writes one JSON-lines
//! file per compilation unit:
//!
//! - an `item` record for every function, method, constant, static, type,
//!   trait, variant and field of the unit: its path, its kind, its file, the
//!   line and one-based column where its definition starts (rustdoc's JSON uses
//!   the same position, which is how the gate binary joins the two), the lines
//!   it spans with its attributes and documentation, the lines its
//!   documentation spans, and a revision: the fingerprint of its source text,
//!   which changes when any byte of the item does. The field `exported` states
//!   whether the crate's public paths reach the item, as the compiler's
//!   effective visibilities decide. An executable has no other crate, so a
//!   reader reads the field of a library's record;
//! - a `clause` record per specified item and clause key: the item's path and
//!   revision, the key, and the sorted paths of the items the clause names;
//! - a `use` record per item that names another item: the item's path and the
//!   sorted paths of the items its body and signature name. A struct or
//!   tuple-struct pattern names each field it binds, and a field it skips with
//!   `_` or a rest is no name.
//!
//! # Dependencies
//!
//! Paths are printed from definition paths with the crate name and without
//! re-export shortening, so a defining crate and its consumers spell an item
//! the same way. An integration test, benchmark or binary is no library:
//! nothing else names its items, and its crate name (`tests`, often) repeats
//! across a workspace's packages, so its own paths carry the package as a
//! `package/` prefix. Modules are not indexed: a module spans everything in it,
//! and a link to one would flag every specification that names it on any change
//! inside. The file name derives from the unit's root file and kind, so
//! recompiling a unit overwrites its own record and an unchanged unit keeps a
//! record that still describes its sources. That holds only if switching the
//! records on or off relints the unit, so every unit lists `DYLINT_TOML`, set
//! or unset, among its environment inputs ([`track_configuration`]): Dylint
//! lists the variable only when it is set, and a unit linted without it would
//! stay fresh for a later run that sets it, which would then write nothing for
//! the unit. A failed write is a compilation error: an evidence run that lost a
//! record must not pass.

use alloc::collections::BTreeMap;
use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::hash::Hash as _;
use core::hash::Hasher as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

use quenchant_shape::shape::Maybe;
use rustc_ast::AssocItem;
use rustc_ast::Attribute;
use rustc_ast::FieldDef;
use rustc_ast::Item;
use rustc_ast::ItemKind;
use rustc_ast::Variant;
use rustc_data_structures::fingerprint::Fingerprint;
use rustc_data_structures::fx::FxHashMap;
use rustc_data_structures::fx::FxIndexMap;
use rustc_data_structures::fx::FxIndexSet;
use rustc_data_structures::stable_hash::StableHasher;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::Pat;
use rustc_hir::PatExprKind;
use rustc_hir::PatKind;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::EarlyContext;
use rustc_lint::EarlyLintPass;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::impl_lint_pass;
use rustc_middle::ty;
use rustc_middle::ty::Instance;
use rustc_middle::ty::InstanceKind;
use rustc_middle::ty::TyCtxt;
use rustc_middle::ty::print::with_no_trimmed_paths;
use rustc_session::Session;
use rustc_span::BytePos;
use rustc_span::FileName;
use rustc_span::Span;
use rustc_span::Symbol;
use rustc_structures::CrateType;

use crate::executable::clause_spans;
use crate::walk_summary::INSTRUMENTATION_CRATE;
use crate::walk_summary::LIBRARY_CRATES;

quenchant_shape::reason_enum! {
    /// A source position can lie outside every authored clause.
    mod clause_lookup {
        /// Why a position belongs to no clause.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Outside {
            /// No recorded clause range encloses the position.
            NoClause,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A resolution can be something other than a named item.
    mod naming {
        /// Why a resolved expression names no dependency.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum NotAnItem {
            /// A local, a parameter, `Self` or a primitive.
            Local,
            /// A definition that is not an item a specification depends on: a
            /// module, a generic parameter, a closure, a macro.
            OtherDefinition,
            /// An item of the standard library or the instrumentation.
            Library,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A definition can have no record of its own.
    mod indexing {
        /// Why a definition is left out of the index.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unindexed {
            /// A module, an implementation block, an import or a generated
            /// definition.
            OtherKind,
            /// The definition has no authored source file to point into.
            NoSource,
        }
    }
}

quenchant_shape::reason_enum! {
    /// The configuration can leave the records off.
    pub mod evidence_setting {
        /// Why no records are written.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unset {
            /// The configuration names no `evidence_directory`.
            NoDirectory,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A compilation unit's own paths can need no qualifier.
    pub mod unit_paths {
        /// Why a unit's paths are printed as the compiler prints them.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unqualified {
            /// A library or its unit tests, whose paths other crates use, or
            /// a unit compiled outside Cargo.
            Library,
        }
    }
}

quenchant_shape::reason_enum! {
    /// A definition can have no body whose behaviour a hash reads.
    pub mod body_hashing {
        /// Why a definition has no body hash.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unhashed {
            /// The definition is no function or method with a body.
            NoBody,
            /// The compiler's printer refuses a part of the body.
            Unprintable,
        }
    }
}

/// The configuration key naming the record directory.
const DIRECTORY_KEY: &str = "evidence_directory";

/// The environment variable Dylint reads its configuration from first.
const CONFIGURATION_VARIABLE: &str = "DYLINT_TOML";

/// The authored clauses, by the start of their value's source range.
#[derive(Clone, Default)]
#[repr(transparent)]
pub struct ClauseIndex(Arc<Mutex<BTreeMap<BytePos, (BytePos, Symbol)>>>);

impl ClauseIndex
{
    /// Record one clause's key and source range.
    ///
    /// # Specification
    /// - ensures: [`locate`](Self::locate) answers `key` for `span` afterwards.
    /// - panics: none; a poisoned lock is recovered, since every write leaves
    ///   the map consistent.
    /// - executable: none — the outcome is the shared map, read through
    ///   [`locate`](Self::locate) by the late pass of the same compilation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's clauses are recorded only
    ///   because their ranges were indexed; the same names in a body are not.
    /// - witness: `tests::ui_evidence`
    fn record(
        &self,
        span: Span,
        key: Symbol,
    )
    {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(span.lo(), (span.hi(), key));
    }

    /// The clause whose range encloses `span`.
    ///
    /// # Specification
    /// - ensures: `Present(key)` exactly when the recorded range starting
    ///   nearest at or before `span` ends at or after it, with that range's
    ///   key; `Absent(Outside::NoClause)` otherwise.
    /// - panics: none; a poisoned lock is recovered.
    /// - executable: none — the map sits behind a lock the predicate would take
    ///   a second time.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture names items in a `requires`
    ///   clause, in an `ensures` clause and in a body; each clause's names are
    ///   recorded under its own key and the body's under none.
    /// - witness: `tests::ui_evidence`
    fn locate(
        &self,
        span: Span,
    ) -> Maybe<Symbol, clause_lookup::Outside>
    {
        let regions = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match regions.range(..= span.lo()).next_back() {
            Some((_, &(end, key))) if span.hi() <= end => Maybe::Present(key),
            _ => Maybe::Absent(clause_lookup::Outside::NoClause),
        }
    }
}

/// Where the authored outer attributes of each item begin, by the start of
/// the item they precede.
#[derive(Clone, Default)]
#[repr(transparent)]
pub struct AttributeIndex(Arc<Mutex<FxHashMap<BytePos, BytePos>>>);

impl AttributeIndex
{
    /// Record where the outer attributes of the node spanning `node` begin.
    ///
    /// # Specification
    /// - ensures: [`extent_start`](Self::extent_start) of `node`'s start
    ///   answers the earliest start among `attrs` that lies before it, when one
    ///   does.
    /// - panics: none; a poisoned lock is recovered, since every write leaves
    ///   the map consistent.
    /// - executable: none — the outcome is the shared map, read through
    ///   [`extent_start`](Self::extent_start) by the late pass of the same
    ///   compilation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's items carry an attribute
    ///   macro, parsed attributes and a derive, some above their documentation;
    ///   each extent starts at its first attribute.
    /// - witness: `tests::ui_evidence`
    fn record(
        &self,
        node: Span,
        attrs: &[Attribute],
    )
    {
        let Some(first) = attrs
            .iter()
            .map(|attr| attr.span.lo())
            .filter(|&start| start < node.lo())
            .min()
        else {
            return;
        };
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(node.lo())
            .and_modify(|start| *start = (*start).min(first))
            .or_insert(first);
    }

    /// Record the attributes of `fields`.
    ///
    /// # Specification
    /// - ensures: [`record`](Self::record) for each field's span and
    ///   attributes.
    /// - panics: none.
    /// - executable: none — the outcome is the shared map.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's documented fields start at
    ///   their documentation.
    /// - witness: `tests::ui_evidence`
    fn record_fields(
        &self,
        fields: &[FieldDef],
    )
    {
        for field in fields {
            self.record(field.span, &field.attrs);
        }
    }

    /// Where the item starting at `start` begins with its attributes.
    ///
    /// # Specification
    /// - ensures: the earliest attribute start recorded for `start` when it
    ///   precedes `start`; `start` itself otherwise.
    /// - panics: none; a poisoned lock is recovered.
    /// - executable: none — the map sits behind a lock the predicate would take
    ///   a second time.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's extents start at the first
    ///   attribute, above the documentation where one is written there.
    /// - witness: `tests::ui_evidence`
    fn extent_start(
        &self,
        start: BytePos,
    ) -> BytePos
    {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&start)
            .map_or(start, |&first| first.min(start))
    }
}

/// Pre-expansion recorder of the authored clauses and attribute positions.
pub struct ClauseRecorder
{
    /// Where the clauses are shared with the late pass.
    index: ClauseIndex,
    /// Where the attribute positions are shared with the late pass.
    attributes: AttributeIndex,
}

impl ClauseRecorder
{
    /// The recorder writes into the indexes its late reader holds.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub const fn new(
        index: ClauseIndex,
        attributes: AttributeIndex,
    ) -> Self
    {
        Self { index, attributes }
    }
}

impl_lint_pass!(ClauseRecorder => []);

impl EarlyLintPass for ClauseRecorder
{
    /// Record each clause an authored `spec` attribute writes.
    ///
    /// # Specification
    /// - ensures: records every clause [`clause_spans`] reads from `attr`;
    ///   ignores every other attribute.
    /// - panics: none.
    /// - executable: none — the outcome is the shared index, observed through
    ///   the late pass's records.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture states clauses directly and
    ///   under `cfg_attr`; a missed form loses its edges.
    /// - witness: `tests::ui_evidence`
    fn check_attribute(
        &mut self,
        _cx: &EarlyContext<'_>,
        attr: &Attribute,
    )
    {
        for clause in clause_spans(attr) {
            self.index.record(clause.span, clause.key);
        }
    }

    /// Record where an item's attributes begin, and its fields' and foreign
    /// items'.
    ///
    /// # Specification
    /// - ensures: records `item`'s attributes, and those of each field of a
    ///   struct or union and each item of a foreign block it declares.
    /// - panics: none.
    /// - executable: none — the outcome is the shared index, observed through
    ///   the late pass's records.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's functions, struct and enum
    ///   carry attributes; a missed item kind starts its extent too late.
    /// - witness: `tests::ui_evidence`
    fn check_item(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &Item,
    )
    {
        self.attributes.record(item.span, &item.attrs);
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ItemKind` is rustc's AST item kind, whose variants move with the pinned \
                      nightly; only a struct, a union or a foreign block holds attributed members"
        )]
        match item.kind {
            ItemKind::Struct(_, _, ref data) | ItemKind::Union(_, _, ref data) => {
                self.attributes.record_fields(data.fields());
            },
            ItemKind::ForeignMod(ref block) => {
                for foreign in &block.items {
                    self.attributes.record(foreign.span, &foreign.attrs);
                }
            },
            _ => {},
        }
    }

    /// Record where a trait item's attributes begin.
    ///
    /// # Specification
    /// - ensures: records `item`'s attributes.
    /// - panics: none.
    /// - executable: none — the outcome is the shared index.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's trait method starts at its
    ///   documentation.
    /// - witness: `tests::ui_evidence`
    fn check_trait_item(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &AssocItem,
    )
    {
        self.attributes.record(item.span, &item.attrs);
    }

    /// Record where an implementation item's attributes begin.
    ///
    /// # Specification
    /// - ensures: records `item`'s attributes.
    /// - panics: none.
    /// - executable: none — the outcome is the shared index.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's methods start at their
    ///   documentation.
    /// - witness: `tests::ui_evidence`
    fn check_impl_item(
        &mut self,
        _cx: &EarlyContext<'_>,
        item: &AssocItem,
    )
    {
        self.attributes.record(item.span, &item.attrs);
    }

    /// Record where a variant's attributes begin, and its fields'.
    ///
    /// # Specification
    /// - ensures: records `variant`'s attributes and each of its fields'.
    /// - panics: none.
    /// - executable: none — the outcome is the shared index.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's variants start at their
    ///   documentation.
    /// - witness: `tests::ui_evidence`
    fn check_variant(
        &mut self,
        _cx: &EarlyContext<'_>,
        variant: &Variant,
    )
    {
        self.attributes.record(variant.span, &variant.attrs);
        self.attributes.record_fields(variant.data.fields());
    }
}

/// Late reader of the names inside authored clauses and item bodies, and
/// writer of the records.
pub struct SpecificationEdges
{
    /// The clauses the pre-expansion pass recorded.
    index: ClauseIndex,
    /// The attribute positions the pre-expansion pass recorded.
    attributes: AttributeIndex,
    /// Where the records are written.
    directory: PathBuf,
    /// The package this unit's own paths are qualified with, when it is not a
    /// library.
    qualifier: Maybe<String, unit_paths::Unqualified>,
    /// Per specified item and clause key, the definitions the clause names.
    named: FxIndexMap<LocalDefId, BTreeMap<Symbol, FxIndexSet<DefId>>>,
    /// Per item, the definitions its body and signature name, itself left out.
    used: FxIndexMap<LocalDefId, FxIndexSet<DefId>>,
}

impl SpecificationEdges
{
    /// The late pass reads the indexes its recorder writes, writes into
    /// `directory`, and qualifies the unit's paths as [`unit_qualifier`]
    /// answers.
    ///
    /// # Specification
    /// trivial.
    #[must_use]
    pub fn new(
        index: ClauseIndex,
        attributes: AttributeIndex,
        directory: PathBuf,
    ) -> Self
    {
        Self {
            index,
            attributes,
            directory,
            qualifier: unit_qualifier(),
            named: FxIndexMap::default(),
            used: FxIndexMap::default(),
        }
    }

    /// Note that a clause of `owner` names `target`.
    ///
    /// # Specification
    /// - ensures: `target` is among the definitions recorded for `owner` under
    ///   `clause`; recording it again changes nothing.
    /// - panics: none.
    /// - executable: none — the outcome is the pass's own accumulated map,
    ///   observed through the records written at the end of the crate.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture names one item twice in one
    ///   clause and once in another; each clause lists it once.
    /// - witness: `tests::ui_evidence`
    fn note(
        &mut self,
        owner: LocalDefId,
        clause: Symbol,
        target: DefId,
    )
    {
        self.named
            .entry(owner)
            .or_default()
            .entry(clause)
            .or_default()
            .insert(target);
    }

    /// Note that the body or signature of `owner` names `target`.
    ///
    /// # Specification
    /// - ensures: `target` is among the definitions recorded for `owner`,
    ///   unless `target` is `owner` itself; recording it again changes nothing.
    /// - panics: none.
    /// - executable: none — the outcome is the pass's own accumulated map,
    ///   observed through the records written at the end of the crate.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the uses fixture's recursive function calls itself
    ///   and another function; its record names only the other.
    /// - witness: `tests::ui_evidence`
    fn note_use(
        &mut self,
        owner: LocalDefId,
        target: DefId,
    )
    {
        if target != owner.to_def_id() {
            self.used.entry(owner).or_default().insert(target);
        }
    }
}

impl_lint_pass!(SpecificationEdges => []);

impl<'tcx> LateLintPass<'tcx> for SpecificationEdges
{
    /// Note the item an expression names.
    ///
    /// # Specification
    /// - ensures: notes the item [`named_item`] resolves for a path or struct
    ///   literal, the method [`resolved_method`] resolves for a method call,
    ///   and the field [`field_item`] resolves for a field read: as a use of
    ///   `expr`'s owner, and under a clause of that owner when `expr`'s call
    ///   site lies inside the clause; notes nothing for any other expression.
    /// - panics: none.
    /// - executable: none — the outcome is the pass's accumulated map, which
    ///   the records written at the end of the crate expose.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's clause calls a function,
    ///   compares with a constant, reads a field and calls a method; the
    ///   function's body calls a fourth item, which is not named. The uses
    ///   fixture calls functions, reads a field and calls methods outside every
    ///   clause, and each of them is a use.
    /// - witness: `tests::ui_evidence`
    fn check_expr(
        &mut self,
        cx: &LateContext<'tcx>,
        expr: &'tcx Expr<'tcx>,
    )
    {
        let owner = expr.hir_id.owner.def_id;
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`ExprKind` is rustc's HIR expression kind, whose variants move with the \
                      pinned nightly; only a path, a struct literal, a method call or a field \
                      names an item"
        )]
        let named = match expr.kind {
            ExprKind::Path(ref qpath) | ExprKind::Struct(&ref qpath, ..) => {
                named_item(cx, cx.qpath_res(qpath, expr.hir_id), expr.hir_id)
            },
            ExprKind::MethodCall(..) => {
                let Some(method) = cx.typeck_results().type_dependent_def_id(expr.hir_id)
                else {
                    return;
                };
                named_item(cx, Res::Def(DefKind::AssocFn, method), expr.hir_id)
            },
            ExprKind::Field(base, _) => field_item(cx, base, expr.hir_id),
            _ => return,
        };
        let Maybe::Present(target) = named
        else {
            return;
        };
        self.note_use(owner, target);
        if let Maybe::Present(clause) = self.index.locate(expr.span.source_callsite()) {
            self.note(owner, clause, target);
        }
    }

    /// Note the items a pattern names.
    ///
    /// # Specification
    /// - ensures: notes the struct, variant or constant a tuple-struct, struct
    ///   or path pattern resolves to, and each field [`pattern_fields`] finds
    ///   the pattern reads: as uses of `pat`'s owner, and under a clause of
    ///   that owner when `pat`'s call site lies inside the clause; notes
    ///   nothing for any other pattern.
    /// - panics: none.
    /// - executable: none — the outcome is the pass's accumulated map, which
    ///   the records written at the end of the crate expose.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's `ensures` clause matches a
    ///   variant and a constant in a pattern; both are named. The uses fixture
    ///   matches a variant outside every clause, and it is a use. It binds a
    ///   struct's field by name beside a rest, and a tuple struct's fields by
    ///   position before and after a rest beside a wildcard: each bound field
    ///   is a use, and the skipped fields are not.
    /// - witness: `tests::ui_evidence`
    fn check_pat(
        &mut self,
        cx: &LateContext<'tcx>,
        pat: &'tcx Pat<'tcx>,
    )
    {
        let owner = pat.hir_id.owner.def_id;
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`PatKind` is rustc's HIR pattern kind, whose variants move with the pinned \
                      nightly; only a struct, tuple-struct or path pattern names an item"
        )]
        let named = match pat.kind {
            PatKind::TupleStruct(ref qpath, ..) | PatKind::Struct(ref qpath, ..) => {
                named_item(cx, cx.qpath_res(qpath, pat.hir_id), pat.hir_id)
            },
            PatKind::Expr(path) => {
                let PatExprKind::Path(ref qpath) = path.kind
                else {
                    return;
                };
                named_item(cx, cx.qpath_res(qpath, path.hir_id), path.hir_id)
            },
            _ => return,
        };
        let Maybe::Present(target) = named
        else {
            return;
        };
        let clause = self.index.locate(pat.span.source_callsite());
        for target in core::iter::once(target).chain(pattern_fields(cx, pat)) {
            self.note_use(owner, target);
            if let Maybe::Present(clause) = clause {
                self.note(owner, clause, target);
            }
        }
    }

    /// Note the item a path names, in a signature, a bound, an
    /// implementation's header or a body.
    ///
    /// # Specification
    /// - ensures: notes as a use of `hir_id`'s owner the item [`named_item`]
    ///   resolves for `path`. It notes nothing for an associated function,
    ///   whose expression's typing resolves it in
    ///   [`check_expr`](Self::check_expr), and nothing for a path of an import
    ///   or a crate import, which names no use.
    /// - panics: none.
    /// - executable: none — the outcome is the pass's accumulated map, which
    ///   the records written at the end of the crate expose.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the uses fixture names a type in a signature, a trait
    ///   in a bound, and both in an implementation's header; each is a use. It
    ///   imports one function and re-exports another, and neither import is a
    ///   use.
    /// - witness: `tests::ui_evidence`
    fn check_path(
        &mut self,
        cx: &LateContext<'tcx>,
        path: &rustc_hir::Path<'tcx>,
        hir_id: HirId,
    )
    {
        if matches!(path.res, Res::Def(DefKind::AssocFn, _)) {
            return;
        }
        let owner = hir_id.owner.def_id;
        if matches!(cx.tcx.def_kind(owner), DefKind::Use | DefKind::ExternCrate) {
            return;
        }
        if let Maybe::Present(target) = named_item(cx, path.res, hir_id) {
            self.note_use(owner, target);
        }
    }

    /// Write the unit's records.
    ///
    /// # Specification
    /// - ensures: writes into the configured directory, under the name
    ///   [`record_name`] gives, one `item` record per definition
    ///   [`item_record`] indexes, then one `clause` record per specified item
    ///   and clause key with the sorted, deduplicated paths of the items noted
    ///   for it, then one `use` record per item noted with a use, with the
    ///   sorted, deduplicated paths of the items it names other than its own
    ///   path, and none where no other path remains; replaces any earlier
    ///   record of the same unit.
    /// - fails: reports a compilation error naming the file when the directory
    ///   cannot be created or the file cannot be written, and one when a record
    ///   cannot be rendered, writing nothing.
    /// - panics: none.
    /// - executable: none — the outcome is a file the gate binary reads; the
    ///   compilation's own result cannot observe it.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture's record is compared whole
    ///   against its pinned expectation, item, clause and use records alike.
    /// - witness: `tests::ui_evidence`
    fn check_crate_post(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        use core::fmt::Write as _;

        let tcx = cx.tcx;
        let mut revisions: FxHashMap<LocalDefId, String> = FxHashMap::default();
        let mut contents = String::new();
        // Rendering a JSON value into a `String` fails only if its `Display`
        // does, which `serde_json` never reports for a `Value`; the first
        // failure is still kept and reported instead of a partial record.
        let mut rendered = Ok(());
        let mut bodies = BodyHasher::new(tcx);
        for def_id in indexed_definitions(tcx) {
            let Maybe::Present(record) =
                item_record(tcx, &mut bodies, &self.attributes, &self.qualifier, def_id)
            else {
                continue;
            };
            if let Some(revision) = record.get("revision").and_then(serde_json::Value::as_str) {
                revisions.insert(def_id, revision.to_owned());
            }
            rendered = rendered.and_then(|()| writeln!(contents, "{record}"));
        }
        // One clause's names, collected into one set every clause reuses.
        let mut names: BTreeSet<String> = BTreeSet::new();
        for (&owner, clauses) in &self.named {
            let revision = revisions.get(&owner).map_or("", String::as_str);
            for (clause, targets) in clauses {
                names.clear();
                names.extend(
                    targets
                        .iter()
                        .map(|&target| canonical_path(tcx, &self.qualifier, target)),
                );
                let record = serde_json::json!({
                    "record": "clause",
                    "item": canonical_path(tcx, &self.qualifier, owner.to_def_id()),
                    "revision": revision,
                    "clause": clause.as_str(),
                    "names": names,
                });
                rendered = rendered.and_then(|()| writeln!(contents, "{record}"));
            }
        }
        // Each named definition's path, printed once for every item that names
        // it.
        let mut paths: FxHashMap<DefId, String> = FxHashMap::default();
        for targets in self.used.values() {
            for &target in targets {
                paths
                    .entry(target)
                    .or_insert_with(|| canonical_path(tcx, &self.qualifier, target));
            }
        }
        // One item's names, borrowed from the printed paths. An inherent
        // implementation block prints as its self type, so its header's name of
        // that type is the block's own path and no use.
        let mut used: BTreeSet<&str> = BTreeSet::new();
        for (&owner, targets) in &self.used {
            let item = canonical_path(tcx, &self.qualifier, owner.to_def_id());
            used.clear();
            used.extend(
                targets
                    .iter()
                    .filter_map(|target| paths.get(target))
                    .map(String::as_str)
                    .filter(|&name| name != item),
            );
            if used.is_empty() {
                continue;
            }
            let record = serde_json::json!({
                "record": "use",
                "item": item,
                "names": used,
            });
            rendered = rendered.and_then(|()| writeln!(contents, "{record}"));
        }
        if rendered.is_err() {
            tcx.dcx()
                .err("quenchant evidence records could not be rendered as JSON");
            return;
        }
        let path = self.directory.join(record_name(tcx));
        let written = std::fs::create_dir_all(&self.directory)
            .and_then(|()| std::fs::write(&path, contents.as_bytes()));
        if let Err(error) = written {
            tcx.dcx().err(format!(
                "quenchant evidence records could not be written to `{}`: {error}",
                path.display()
            ));
        }
    }
}

/// Read the record directory from the Dylint configuration.
///
/// # Specification
/// - requires: `dylint_linting::init_config` has already run, which
///   [`crate::register_lints`] guarantees by calling it first.
/// - ensures: `Present` with the configured path exactly when the
///   `quenchant-dylints` table holds a string `evidence_directory`.
/// - provides: `evidence_setting::Unset::NoDirectory` when the configuration is
///   absent, unreadable, or names no directory: the records stay off.
/// - panics: none.
/// - executable: none — the configuration read outcome is not retained; reading
///   it again is a distinct fallible operation.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture configures the directory and its
///   records appear there; every other UI suite configures none and writes
///   nothing.
/// - witness: `tests::ui_evidence`
pub fn evidence_directory() -> Maybe<PathBuf, evidence_setting::Unset>
{
    let Ok(Some(config)) = dylint_linting::config_toml(env!("CARGO_PKG_NAME"))
    else {
        return Maybe::Absent(evidence_setting::Unset::NoDirectory);
    };
    config
        .get(DIRECTORY_KEY)
        .and_then(|value| value.as_str())
        .map_or(
            Maybe::Absent(evidence_setting::Unset::NoDirectory),
            |directory| Maybe::Present(PathBuf::from(directory)),
        )
}

/// List the configuration variable among the unit's environment inputs, set or
/// unset.
///
/// # Specification
/// - ensures: the session's environment dependencies hold `DYLINT_TOML` with
///   its current value, or with none when it is unset or not Unicode, so Cargo
///   relints the unit when the variable appears, changes or goes.
/// - panics: none.
/// - executable: none — the dependency set reaches only the dep-info file rustc
///   writes for Cargo.
///
/// # Adequacy
/// - hypothesis: L2 — a unit linted without the variable lists it, unset, in
///   the dep-info rustc writes.
/// - witness: `tests::ui_tracking`
pub fn track_configuration(sess: &Session)
{
    let value = std::env::var(CONFIGURATION_VARIABLE).ok();
    sess.env_depinfo.lock().insert((
        Symbol::intern(CONFIGURATION_VARIABLE),
        value.as_deref().map(Symbol::intern),
    ));
}

/// The definition a resolution names, as a dependency of a clause.
///
/// # Specification
/// - requires: `hir_id` is the node `res` was resolved at, inside the body
///   whose typeck results `cx` holds.
/// - ensures: a constructor names its struct or variant; a method or associated
///   function names the implementation [`resolved_method`] finds; a function,
///   constant, static, type, trait, variant or field names itself.
/// - provides: `naming::NotAnItem::Local` for a resolution that is not a
///   definition, `OtherDefinition` for a definition of another kind, and
///   `Library` for a definition in `core`, `alloc`, `std` or the
///   instrumentation.
/// - panics: none.
/// - executable: none — the outcome is the compiler's resolution, which a
///   predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture names a function, a constant, a
///   variant through its constructor and a method beside a local, a parameter
///   and a standard-library method; only the first four are recorded.
/// - witness: `tests::ui_evidence`
fn named_item(
    cx: &LateContext<'_>,
    res: Res,
    hir_id: HirId,
) -> Maybe<DefId, naming::NotAnItem>
{
    let Res::Def(kind, def_id) = res
    else {
        return Maybe::Absent(naming::NotAnItem::Local);
    };
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`DefKind` is rustc's definition kind, whose variants move with the pinned \
                  nightly; only the named kinds are items a clause can mention"
    )]
    let target = match kind {
        DefKind::Ctor(..) => cx.tcx.parent(def_id),
        DefKind::AssocFn => resolved_method(cx, def_id, hir_id),
        DefKind::Fn
        | DefKind::Const
        | DefKind::AssocConst
        | DefKind::Static { .. }
        | DefKind::Struct
        | DefKind::Union
        | DefKind::Enum
        | DefKind::Variant
        | DefKind::Trait
        | DefKind::TraitAlias
        | DefKind::TyAlias
        | DefKind::ForeignTy
        | DefKind::AssocTy
        | DefKind::Field => def_id,
        _ => return Maybe::Absent(naming::NotAnItem::OtherDefinition),
    };
    let krate = cx.tcx.crate_name(target.krate);
    if LIBRARY_CRATES.contains(&krate.as_str()) || krate.as_str() == INSTRUMENTATION_CRATE {
        return Maybe::Absent(naming::NotAnItem::Library);
    }
    Maybe::Present(target)
}

/// The implementation a call of an associated function reaches.
///
/// # Specification
/// - requires: `method` is an associated function and `hir_id` the node its
///   call or path was typed at.
/// - ensures: a trait method whose instance resolves, under the call's generic
///   arguments, to one implementation's item names that item; an inherent
///   method, and a trait method resolving to no single body, names `method`
///   itself.
/// - panics: none.
/// - executable: none — the outcome is the compiler's instance resolution,
///   which a predicate could only repeat.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture calls a trait method on a concrete
///   type in a clause; the implementation's method is recorded, not the trait's
///   declaration.
/// - witness: `tests::ui_evidence`
fn resolved_method(
    cx: &LateContext<'_>,
    method: DefId,
    hir_id: HirId,
) -> DefId
{
    if cx.tcx.trait_of_assoc(method).is_none() {
        return method;
    }
    let args = cx.typeck_results().node_args(hir_id);
    if cx.tcx.generics_of(method).count() != args.len() {
        return method;
    }
    let args = cx.tcx.erase_and_anonymize_regions(args);
    match Instance::try_resolve(cx.tcx, cx.typing_env(), method, args) {
        #[expect(
            clippy::wildcard_enum_match_arm,
            reason = "`InstanceKind` is rustc's instance kind, whose variants move with the \
                      pinned nightly; only a resolved non-closure item replaces the method"
        )]
        Ok(Some(instance)) => match instance.def {
            InstanceKind::Item(target) if !cx.tcx.is_closure_like(target) => target,
            _ => method,
        },
        _ => method,
    }
}

/// The field a field read names.
///
/// # Specification
/// - requires: `hir_id` is the field expression whose receiver is `base`.
/// - ensures: a read of a struct's or union's field, after the receiver's
///   adjustments, names that field's definition, subject to the library
///   exclusion [`named_item`] applies.
/// - provides: `naming::NotAnItem::Local` when the receiver is not a struct or
///   union or the read has no recorded field index.
/// - panics: none.
/// - executable: none — the outcome is the typeck results' field index, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture's `ensures` clause reads a field of
///   the returned struct; the field is recorded.
/// - witness: `tests::ui_evidence`
fn field_item(
    cx: &LateContext<'_>,
    base: &Expr<'_>,
    hir_id: HirId,
) -> Maybe<DefId, naming::NotAnItem>
{
    let typeck = cx.typeck_results();
    let ty::Adt(adt, _) = *typeck.expr_ty_adjusted(base).peel_refs().kind()
    else {
        return Maybe::Absent(naming::NotAnItem::Local);
    };
    if adt.is_enum() {
        return Maybe::Absent(naming::NotAnItem::Local);
    }
    let Some(index) = typeck.opt_field_index(hir_id)
    else {
        return Maybe::Absent(naming::NotAnItem::Local);
    };
    let Some(field) = adt.non_enum_variant().fields.get(index)
    else {
        return Maybe::Absent(naming::NotAnItem::Local);
    };
    named_item(cx, Res::Def(DefKind::Field, field.did), hir_id)
}

/// The fields a struct or tuple-struct pattern reads.
///
/// # Specification
/// - requires: `pat` lies inside the body whose typeck results `cx` holds.
/// - ensures: a struct pattern reads each field it lists with a pattern other
///   than `_`; a tuple-struct pattern reads the field at each position whose
///   pattern is not `_`, a position after a rest counted from the variant's
///   last field. Each field passes the library exclusion [`named_item`]
///   applies. Any other pattern, and a pattern whose type is no struct, union
///   or enum, reads no field.
/// - panics: none.
/// - executable: none — the outcome is the typeck results' field indices and
///   the variant's own fields, which a predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the uses fixture binds a struct's field by name beside a
///   rest, and a tuple struct's fields by position before and after a rest
///   beside a wildcard; each bound field is recorded, and no skipped one is.
/// - witness: `tests::ui_evidence`
fn pattern_fields(
    cx: &LateContext<'_>,
    pat: &Pat<'_>,
) -> Vec<DefId>
{
    let typeck = cx.typeck_results();
    let ty::Adt(adt, _) = *typeck.pat_ty(pat).peel_refs().kind()
    else {
        return Vec::new();
    };
    let read = |field: Option<&ty::FieldDef>, part: &Pat<'_>| {
        let field = field.filter(|_| !matches!(part.kind, PatKind::Wild))?;
        match named_item(cx, Res::Def(DefKind::Field, field.did), pat.hir_id) {
            Maybe::Present(target) => Some(target),
            Maybe::Absent(_) => None,
        }
    };
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`PatKind` is rustc's HIR pattern kind, whose variants move with the pinned \
                  nightly; only a struct or tuple-struct pattern binds fields"
    )]
    match pat.kind {
        PatKind::Struct(ref qpath, fields, _) => {
            let variant = adt.variant_of_res(cx.qpath_res(qpath, pat.hir_id));
            fields
                .iter()
                .filter_map(|field| {
                    let index = typeck.opt_field_index(field.hir_id)?;
                    read(variant.fields.get(index), field.pat)
                })
                .collect()
        },
        PatKind::TupleStruct(ref qpath, parts, rest) => {
            let variant = adt.variant_of_res(cx.qpath_res(qpath, pat.hir_id));
            let skipped = variant.fields.len().saturating_sub(parts.len());
            let after = rest.as_opt_usize().unwrap_or(usize::MAX);
            parts
                .iter()
                .enumerate()
                .filter_map(|(position, part)| {
                    let position = if position >= after {
                        position.saturating_add(skipped)
                    }
                    else {
                        position
                    };
                    read(variant.fields.raw.get(position), part)
                })
                .collect()
        },
        _ => Vec::new(),
    }
}

/// Every local definition that can carry an item record, in definition order.
///
/// # Specification
/// - ensures: yields every HIR owner of the crate, then, for each local struct,
///   union and enum among them, its variants that are not the type itself and
///   every field.
/// - provides: the candidates [`item_record`] decides on.
/// - panics: none.
/// - executable: none — the outcome is the crate's own definition table.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture's record holds its enum's variants
///   and its struct's field beside the owners.
/// - witness: `tests::ui_evidence`
fn indexed_definitions(tcx: TyCtxt<'_>) -> Vec<LocalDefId>
{
    let mut definitions = Vec::new();
    for owner in tcx.hir_crate_items(()).definitions() {
        definitions.push(owner);
        if !matches!(
            tcx.def_kind(owner),
            DefKind::Struct | DefKind::Union | DefKind::Enum
        ) {
            continue;
        }
        let adt = tcx.adt_def(owner);
        for variant in adt.variants() {
            if let Some(local) = variant.def_id.as_local()
                && local != owner
            {
                definitions.push(local);
            }
            definitions.extend(
                variant
                    .fields
                    .iter()
                    .filter_map(|field| field.did.as_local()),
            );
        }
    }
    definitions
}

/// The `item` record of one definition.
///
/// # Specification
/// - ensures: a function, method, constant, static, type, trait, variant or
///   field with authored source yields its path, its kind, file, the line and
///   one-based column where its definition starts, the first and last line of
///   its extent from its first authored attribute or documentation line, the
///   lines its documentation spans or `null`, the fingerprint of the extent's
///   source text as 32 hexadecimal digits, and whether the crate's effective
///   visibilities export the definition. A function or method with MIR also
///   yields `body`, its [`BodyHasher::body`]. The kind is one of `fn`,
///   `assoc-fn`, `const`, `assoc-const`, `static`, `struct`, `union`, `enum`,
///   `variant`, `field`, `trait`, `trait-alias`, `type-alias`, `foreign-type`
///   and `assoc-type`.
/// - provides: `indexing::Unindexed::OtherKind` for every other definition and
///   `NoSource` for one whose span is generated or lies in no local file.
/// - panics: none.
/// - executable: none — the record is an assembled JSON value whose fields the
///   gate binary interprets.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture's record pins each item's position,
///   extent, documentation lines and revision; the same position is where
///   rustdoc's JSON places the item. The uses fixture's public, re-exported and
///   crate-visible items differ in `exported`, and its functions, methods,
///   types, trait, variants and fields each state their kind.
/// - witness: `tests::ui_evidence`
fn item_record(
    tcx: TyCtxt<'_>,
    bodies: &mut BodyHasher<'_>,
    attributes: &AttributeIndex,
    qualifier: &Maybe<String, unit_paths::Unqualified>,
    def_id: LocalDefId,
) -> Maybe<serde_json::Value, indexing::Unindexed>
{
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "`DefKind` is rustc's definition kind, whose variants move with the pinned \
                  nightly; only the named kinds are indexed"
    )]
    let kind = match tcx.def_kind(def_id) {
        DefKind::Fn => "fn",
        DefKind::AssocFn => "assoc-fn",
        DefKind::Const => "const",
        DefKind::AssocConst => "assoc-const",
        DefKind::Static { .. } => "static",
        DefKind::Struct => "struct",
        DefKind::Union => "union",
        DefKind::Enum => "enum",
        DefKind::Variant => "variant",
        DefKind::Field => "field",
        DefKind::Trait => "trait",
        DefKind::TraitAlias => "trait-alias",
        DefKind::TyAlias => "type-alias",
        DefKind::ForeignTy => "foreign-type",
        DefKind::AssocTy => "assoc-type",
        _ => return Maybe::Absent(indexing::Unindexed::OtherKind),
    };
    let source_map = tcx.sess.source_map();
    let hir_id = tcx.local_def_id_to_hir_id(def_id);
    let body = tcx.hir_span_with_body(hir_id);
    if body.is_dummy() || body.in_external_macro(source_map) {
        return Maybe::Absent(indexing::Unindexed::NoSource);
    }
    let head = source_map.lookup_char_pos(body.lo());
    let FileName::Real(ref real) = head.file.name
    else {
        return Maybe::Absent(indexing::Unindexed::NoSource);
    };
    let Some(file) = real.local_path()
    else {
        return Maybe::Absent(indexing::Unindexed::NoSource);
    };
    let mut start = attributes.extent_start(body.lo());
    let mut docs: Option<(BytePos, BytePos)> = None;
    for attr in tcx.hir_attrs(hir_id) {
        let Some(span) = attr.is_doc_comment()
        else {
            continue;
        };
        if span.is_dummy() || !head.file.contains(span.lo()) || span.lo() > body.lo() {
            continue;
        }
        start = start.min(span.lo());
        docs = Some(docs.map_or_else(
            || (span.lo(), span.hi()),
            |(lo, hi)| (lo.min(span.lo()), hi.max(span.hi())),
        ));
    }
    let line = |position: BytePos| source_map.lookup_char_pos(position).line;
    let extent = body.with_lo(start);
    let mut hasher = StableHasher::new();
    source_map
        .span_to_snippet(extent)
        .unwrap_or_default()
        .hash(&mut hasher);
    let (high, low) = hasher.finish::<Fingerprint>().split();
    let mut record = serde_json::json!({
        "record": "item",
        "path": canonical_path(tcx, qualifier, def_id.to_def_id()),
        "kind": kind,
        "file": file,
        "line": head.line,
        "column": head.col.0.saturating_add(1_usize),
        "start": line(start),
        "end": line(body.hi()),
        "docs": docs.map(|(lo, hi)| [line(lo), line(hi)]),
        "revision": format!("{:016x}{:016x}", high.as_u64(), low.as_u64()),
        "exported": tcx.effective_visibilities(()).is_exported(def_id),
    });
    if let Maybe::Present(hash) = bodies.body(def_id)
        && let Some(fields) = record.as_object_mut()
    {
        fields.insert("body".to_owned(), serde_json::Value::String(hash));
    }
    Maybe::Present(record)
}

/// The version of the canonical text of [`BodyHasher`]. A change of the text
/// that an unchanged body prints raises it, so that no base of the earlier text
/// reads as changed.
const TEXT_VERSION: &str = "mir-text-1";

/// The scheme of each body hash that this library writes: eight hexadecimal
/// digits of the version of the canonical text and of the compiler.
///
/// Two compilers can print one body in two texts. So a hash is comparable only
/// with a hash of the same scheme, and a reader of a base takes no base whose
/// hashes carry another scheme.
///
/// # Specification
/// - ensures: eight hexadecimal digits that depend only on [`TEXT_VERSION`] and
///   on the version string of the running compiler, with its commit.
/// - panics: none.
/// - executable: none — the outcome is a hash of the compiler's version, which
///   a predicate could only compute again.
///
/// # Adequacy
/// - hypothesis: L3 — the base fixture writes a record and reads it under one
///   compiler, and it reads a record whose hashes carry another scheme.
/// - witness: `tests::ui_allocation_base`
/// - witness: `tests::ui_allocation_base_unread`
pub fn body_scheme() -> String
{
    let mut hasher = StableHasher::new();
    TEXT_VERSION.hash(&mut hasher);
    rustc_interface::util::rustc_version_str()
        .unwrap_or_default()
        .hash(&mut hasher);
    let (high, _) = hasher.finish::<Fingerprint>().split();
    format!("{:08x}", high.as_u64() >> 32_u32)
}

/// The hash of each function's behaviour, from a canonical text of its MIR.
///
/// The text is the function's argument count, the type of each local, and
/// each statement and terminator of each block. The closures that the
/// function defines follow it. Paths print whole and by crate name. Each
/// allocation prints by its order of first use in the function, so no
/// session's numbering enters the hash. The span that a closure's type prints
/// after `@` is dropped.
///
/// The compiler's stable hash is no fit. It names a definition by a hash that
/// holds the crate's `StableCrateId`, which moves with `-C metadata`. Its
/// caches of lists and ADTs serve every hashing context, so a context of this
/// crate's own still takes their fingerprints.
pub struct BodyHasher<'tcx>
{
    /// The compiler's context.
    tcx: TyCtxt<'tcx>,
    /// The text of one statement, kept for the next.
    line: String,
    /// The order of first use of each allocation in one function.
    allocations: FxHashMap<u64, usize>,
    /// The scheme that each hash carries, [`body_scheme`].
    scheme: String,
}

impl<'tcx> BodyHasher<'tcx>
{
    /// A hasher with no text yet.
    ///
    /// # Specification
    /// trivial.
    pub fn new(tcx: TyCtxt<'tcx>) -> Self
    {
        Self {
            tcx,
            line: String::new(),
            allocations: FxHashMap::default(),
            scheme: body_scheme(),
        }
    }

    /// The hash of a function's behaviour: the canonical text of its
    /// optimized MIR and of its closures' MIR.
    ///
    /// # Specification
    /// - ensures: for a function or method with MIR, the scheme of
    ///   [`body_scheme`], a hyphen, and the stable hash of the canonical text
    ///   as 32 hexadecimal digits. A move of the function's text, an edit of
    ///   its comments and documentation, a new `-C metadata` and a new order of
    ///   the session's allocations keep the hash. A change of a statement, a
    ///   call, a type or a closure changes it.
    /// - provides: `body_hashing::Unhashed::NoBody` for every other definition,
    ///   and `Unprintable` when the compiler's printer refuses a part of the
    ///   body.
    /// - panics: none.
    /// - executable: none — the hash is a hash of the compiler's printing,
    ///   which a predicate could only compute again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the evidence fixture pins the hash of each function;
    ///   the base-record fixture compiles twice under two values of `-C
    ///   metadata`, holds a function whose pinned hash it changes, and keeps
    ///   one whose hash it leaves.
    /// - witness: `tests::ui_evidence`
    /// - witness: `tests::ui_allocation_base`
    pub fn body(
        &mut self,
        def_id: LocalDefId,
    ) -> Maybe<String, body_hashing::Unhashed>
    {
        if !matches!(self.tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn)
            || !self.tcx.is_mir_available(def_id.to_def_id())
        {
            return Maybe::Absent(body_hashing::Unhashed::NoBody);
        }
        self.allocations.clear();
        let mut hasher = StableHasher::new();
        let mut written = self.write_body(&mut hasher, def_id);
        for nested in self.tcx.nested_bodies_within(def_id) {
            if written.is_ok()
                && matches!(self.tcx.def_kind(nested), DefKind::Closure)
                && self.tcx.is_mir_available(nested.to_def_id())
            {
                written = self.write_body(&mut hasher, nested);
            }
        }
        if written.is_err() {
            return Maybe::Absent(body_hashing::Unhashed::Unprintable);
        }
        let (high, low) = hasher.finish::<Fingerprint>().split();
        Maybe::Present(format!(
            "{}-{:016x}{:016x}",
            self.scheme,
            high.as_u64(),
            low.as_u64()
        ))
    }

    /// Hash the canonical text of one body.
    ///
    /// # Specification
    /// - ensures: the argument count and each local's type form one line; then
    ///   each statement forms one line, and each terminator one line after its
    ///   block's name. Each line enters `hasher` through [`BodyHasher::feed`],
    ///   with paths untrimmed.
    /// - fails: the error of the compiler's printer, at the first line it
    ///   refuses.
    /// - panics: none.
    /// - executable: none — the text is the compiler's printing, which a
    ///   predicate could only produce again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — as [`BodyHasher::body`].
    /// - witness: `tests::ui_evidence`
    /// - witness: `tests::ui_allocation_base`
    fn write_body(
        &mut self,
        hasher: &mut StableHasher,
        def_id: LocalDefId,
    ) -> core::fmt::Result
    {
        use core::fmt::Write as _;

        let body = self.tcx.optimized_mir(def_id);
        self.line.clear();
        with_no_trimmed_paths!(write!(self.line, "{}(", body.arg_count))?;
        for declaration in &body.local_decls {
            with_no_trimmed_paths!(write!(self.line, "{},", declaration.ty))?;
        }
        self.feed(hasher);
        for (block, data) in body.basic_blocks.iter_enumerated() {
            for statement in &data.statements {
                self.line.clear();
                with_no_trimmed_paths!(write!(self.line, "{statement:?}"))?;
                self.feed(hasher);
            }
            if let Some(ref terminator) = data.terminator {
                self.line.clear();
                with_no_trimmed_paths!(write!(self.line, "{block:?}: {:?}", terminator.kind))?;
                self.feed(hasher);
            }
        }
        Ok(())
    }

    /// Hash the line of text, with each allocation renumbered and each span
    /// after `@` dropped.
    ///
    /// # Specification
    /// - ensures: the line enters `hasher` as written, then a line end, except
    ///   in two forms. `alloc` followed by digits, at the start of a word,
    ///   enters as `alloc` and the order of the number's first use in this
    ///   function. An `@` enters alone, and the text after it up to the next
    ///   `}` does not enter.
    /// - panics: none.
    /// - executable: none — the outcome is the hasher's state, which a
    ///   predicate could only compute again.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the base fixture compiles under two values of `-C
    ///   metadata`, and the evidence fixture's closure and constants hash the
    ///   same across two runs.
    /// - witness: `tests::ui_evidence`
    /// - witness: `tests::ui_allocation_base`
    fn feed(
        &mut self,
        hasher: &mut StableHasher,
    )
    {
        let mut rest = self.line.as_str();
        while let Some(at) = rest.find(['@', 'a']) {
            let (head, tail) = rest.split_at(at);
            hasher.write(head.as_bytes());
            if let Some(after) = tail.strip_prefix('@') {
                hasher.write(b"@");
                rest = after
                    .find('}')
                    .and_then(|end| after.get(end ..))
                    .unwrap_or_default();
            }
            else if let Some(after) = tail.strip_prefix("alloc")
                && !head
                    .ends_with(|character: char| character.is_alphanumeric() || character == '_')
                && let Some(digits) = after
                    .find(|character: char| !character.is_ascii_digit())
                    .or(Some(after.len()))
                    .filter(|&digits| digits > 0)
                && let Some(number) = after
                    .get(.. digits)
                    .and_then(|text| text.parse::<u64>().ok())
            {
                let next = self.allocations.len();
                let order = *self.allocations.entry(number).or_insert(next);
                hasher.write(b"alloc");
                hasher.write_usize(order);
                rest = after.get(digits ..).unwrap_or_default();
            }
            else {
                hasher.write(b"a");
                rest = tail.get(1 ..).unwrap_or_default();
            }
        }
        hasher.write(rest.as_bytes());
        hasher.write(b"\n");
    }
}

/// The definition path an item record and a clause record spell.
///
/// # Specification
/// - ensures: the path rustc prints for `def_id` with the defining crate's name
///   as its first segment, through the definition's own module rather than a
///   re-export, and untrimmed; an associated item of an inherent implementation
///   prints under its self type. A definition of this unit is prefixed with
///   `package/` when `qualifier` names a package.
/// - provides: the join key between a defining crate's item records and its
///   consumers' clause records.
/// - panics: none.
/// - executable: none — the outcome is the compiler's path printer, which a
///   predicate could only run again.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture's pinned record spells every path
///   with the fixture's crate name, including a method and a variant.
/// - witness: `tests::ui_evidence`
pub fn canonical_path(
    tcx: TyCtxt<'_>,
    qualifier: &Maybe<String, unit_paths::Unqualified>,
    def_id: DefId,
) -> String
{
    let path = rustc_middle::ty::print::with_no_trimmed_paths!(
        rustc_middle::ty::print::with_no_visible_paths!(
            rustc_middle::ty::print::with_resolve_crate_name!(tcx.def_path_str(def_id))
        )
    );
    match *qualifier {
        Maybe::Present(ref package) if def_id.is_local() => format!("{package}/{path}"),
        Maybe::Present(_) | Maybe::Absent(_) => path,
    }
}

/// The package that qualifies the paths of a non-library compilation unit.
///
/// # Specification
/// - ensures: `Present` with Cargo's package name when Cargo compiles an
///   integration test, a benchmark or a binary, which it marks by setting
///   `CARGO_TARGET_TMPDIR` for the first two and `CARGO_BIN_NAME` for the last.
/// - provides: `unit_paths::Unqualified::Library` for a library, a library's
///   unit tests and a compilation outside Cargo.
/// - panics: none.
/// - executable: none — the outcome is the compiler's environment, which a
///   predicate could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture, compiled outside Cargo, keeps its
///   crate name unqualified in every pinned path.
/// - witness: `tests::ui_evidence`
pub fn unit_qualifier() -> Maybe<String, unit_paths::Unqualified>
{
    let library = std::env::var_os("CARGO_TARGET_TMPDIR").is_none()
        && std::env::var_os("CARGO_BIN_NAME").is_none();
    match std::env::var("CARGO_PKG_NAME") {
        Ok(package) if !library => Maybe::Present(package),
        _ => Maybe::Absent(unit_paths::Unqualified::Library),
    }
}

/// The file name of a compilation unit's record.
///
/// # Specification
/// - ensures: the unit's root file path with every character outside ASCII
///   letters, digits, `-` and `_` replaced by `_`, then `.test` for a test
///   harness or the first crate type otherwise, then `.jsonl`; two units of one
///   workspace differ in root file or kind, so their names differ.
/// - panics: none.
/// - executable: none — the name is derived from the session, which a predicate
///   could only read again.
///
/// # Adequacy
/// - hypothesis: L3 — the evidence fixture's record is found under the name its
///   root file gives.
/// - witness: `tests::ui_evidence`
pub fn record_name(tcx: TyCtxt<'_>) -> String
{
    let root = tcx
        .sess
        .local_crate_source_file()
        .and_then(|file| file.local_path().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from(tcx.crate_name(rustc_hir::def_id::LOCAL_CRATE).as_str()));
    let mut name: String = root
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            }
            else {
                '_'
            }
        })
        .collect();
    name.push('.');
    if tcx.sess.is_test_crate() {
        name.push_str("test");
    }
    else {
        name.push_str(
            tcx.crate_types()
                .first()
                .map_or("crate", |crate_type| match *crate_type {
                    CrateType::Executable => "bin",
                    CrateType::Dylib => "dylib",
                    CrateType::Rlib => "lib",
                    CrateType::StaticLib => "staticlib",
                    CrateType::Cdylib => "cdylib",
                    CrateType::ProcMacro => "proc-macro",
                    CrateType::Sdylib => "sdylib",
                }),
        );
    }
    name.push_str(".jsonl");
    name
}
