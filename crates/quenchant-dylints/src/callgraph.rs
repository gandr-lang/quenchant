//! The crate-local call graph and the input-provenance analysis built on it.
//!
//! [`crate::RECURSION_FORBIDDEN`] is a call-plane gate: it needs the crate's
//! own call edges, the strongly connected components over them, and — for the
//! `- input recursion: none.` claim — whether any edge inside a component
//! carries data derived from the caller's own parameters.
//!
//! The visitors opt into `nested_filter::OnlyBodies` so a call written inside a
//! closure counts as a call by the function that builds the closure. That is an
//! over-approximation — the closure may never be invoked — and it is the safe
//! direction for a ban with an explicit escape hatch.
//!
//! Known blind spots of this plane, recorded rather than solved here:
//! derived-trait recursion routes through non-local `std` generics and produces
//! no crate-local edge, compiler-generated drop glue has no HIR function at
//! all, and a callee reached through a function *pointer* has had its
//! definition id erased by the coercion — a function *item* held in a binding
//! still carries its `FnDef` type and is resolved. The type-plane gate is the
//! complementary closure.

use std::collections::HashMap;
use std::collections::HashSet;

use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    /// The local call graph records only recoverable function identities.
    mod call_resolution {
        /// Why a callee supplies no local edge.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The definition belongs to another crate.
            NonLocal,
            /// Type checking recorded no type for this callee.
            TypeUnavailable,
            /// The callee type is not a function item, as with pointers or closures.
            NotFunctionDefinition,
            /// A function-item type names something other than a function or method.
            NotFunctionOrMethod,
        }
    }
}

quenchant_shape::reason_enum! {
    /// Provenance inspection requires an expression rather than any HIR node.
    mod expression_lookup {
        /// Why an id cannot be inspected as an expression.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Missing {
            /// The resolved HIR node has a different kind.
            NotExpression,
        }
    }
}

use rustc_hir::Body;
use rustc_hir::Expr;
use rustc_hir::ExprKind;
use rustc_hir::HirId;
use rustc_hir::LetStmt;
use rustc_hir::Node;
use rustc_hir::Pat;
use rustc_hir::PatKind;
use rustc_hir::def::DefKind;
use rustc_hir::def::Res;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::Visitor;
use rustc_hir::intravisit::walk_expr;
use rustc_hir::intravisit::walk_local;
use rustc_hir::intravisit::walk_pat;
use rustc_lint::LateContext;
use rustc_middle::hir::nested_filter;
use rustc_middle::ty as rustc_ty;
use rustc_middle::ty::TyCtxt;
use rustc_span::Span;

use crate::graph::tarjan_components;
use crate::semantic::ContainsDerivedBinding;
use crate::semantic::HasInputDerivedRecursiveCall;
use crate::semantic::ProvenanceChanged;
use crate::semantic::Vertex;

/// Crate-local function metadata needed for crate-post recursion diagnostics.
pub struct FunctionNode
{
    /// The function's whole-item span, used as the diagnostic target.
    pub span: Span,
    /// The function's stable rustc def-path string, used for deterministic
    /// component and diagnostic ordering.
    pub path: String,
    /// The HIR id of the function body expression root.
    pub body_hir_id: HirId,
    /// HIR ids of the function's parameter pattern bindings.
    pub input_bindings: Vec<HirId>,
}

/// One crate-local callsite, preserving the callee and argument HIR roots.
pub struct CallEdge
{
    /// The callee's crate-local definition id.
    pub callee: LocalDefId,
    /// HIR ids of the argument expressions, receiver first for method calls.
    pub args: Vec<HirId>,
}

/// Collect crate-local free-function and method callsites under `expr`.
///
/// # Specification
/// - requires: `expr` is the body root of the function currently being visited,
///   so `cx.typeck_results()` covers it and every closure body nested in it.
/// - ensures: the returned edges appear in HIR visit order, and include calls
///   written inside nested closure bodies, which are attributed to the function
///   that builds the closure.
/// - provides: the outgoing edge list of one call-graph node.
/// - panics: none.
pub fn local_call_edges<'tcx>(
    cx: &LateContext<'tcx>,
    expr: &'tcx Expr<'tcx>,
) -> Vec<CallEdge>
{
    let mut collector = LocalCallCollector {
        cx,
        calls: Vec::new(),
    };
    collector.visit_expr(expr);
    collector.calls
}

/// HIR visitor that records direct local function/method callsites.
struct LocalCallCollector<'cx, 'tcx>
{
    /// The late lint context used for path and method resolution.
    cx: &'cx LateContext<'tcx>,
    /// The callsites recorded so far, in visit order.
    calls: Vec<CallEdge>,
}

#[expect(
    clippy::renamed_function_params,
    reason = "rustc declares the `Visitor` methods with single-letter parameter names (`ex`, `l`, \
              `p`); the implementation keeps descriptive names"
)]
impl<'tcx> Visitor<'tcx> for LocalCallCollector<'_, 'tcx>
{
    type MaybeTyCtxt = TyCtxt<'tcx>;
    type NestedFilter = nested_filter::OnlyBodies;

    /// Hand rustc's context to the walk.
    ///
    /// # Specification
    /// trivial.
    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt
    {
        self.cx.tcx
    }

    /// Record a callsite and descend into the expression's children.
    ///
    /// # Specification
    /// - ensures: records one edge per resolved crate-local call or method
    ///   call, with its argument nodes, and then walks the expression's
    ///   children.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is the only supported traversal of
    ///   the borrowed HIR arena, and its `walk_expr`/`visit_expr` pair is
    ///   mutually recursive by construction.
    /// - measure: the height of the remaining HIR subtree below `expr`.
    /// - boundedness: HIR expression trees are finite and their depth is capped
    ///   by the parser's own nesting limit, which rejects deeper input before a
    ///   lint pass ever runs.
    /// - input recursion: none.
    fn visit_expr(
        &mut self,
        expr: &'tcx Expr<'_>,
    )
    {
        match expr.kind {
            | ExprKind::Call(callee, args) => {
                if let Maybe::Present(local_def_id) = call_target(self.cx, callee) {
                    self.calls.push(CallEdge {
                        callee: local_def_id,
                        args: args.iter().map(|arg| arg.hir_id).collect(),
                    });
                }
            },
            | ExprKind::MethodCall(_, receiver, args, _) => {
                if let Some(def_id) = self.cx.typeck_results().type_dependent_def_id(expr.hir_id)
                    && let Some(local_def_id) = def_id.as_local()
                {
                    let mut call_args = Vec::with_capacity(args.len().saturating_add(1_usize));
                    call_args.push(receiver.hir_id);
                    call_args.extend(args.iter().map(|arg| arg.hir_id));
                    self.calls.push(CallEdge {
                        callee: local_def_id,
                        args: call_args,
                    });
                }
            },
            | _ => {},
        }
        walk_expr(self, expr);
    }
}

/// Resolve a call's callee expression to a crate-local function definition.
///
/// # Specification
/// - requires: `callee` is the callee position of a `Call` expression in a body
///   covered by `cx.typeck_results()`.
/// - ensures: resolves a direct path to a function or associated function, and
///   otherwise falls back to the callee's own type, which stays a `FnDef`
///   whenever a function item reaches the call position through a binding.
/// - provides: one call-graph edge target.
/// - provides: `call_resolution::Missing` distinguishes `NonLocal` ownership,
///   `TypeUnavailable` HIR, a `NotFunctionDefinition` callee such as a pointer
///   or closure, and a `NotFunctionOrMethod` item such as a constructor.
/// - panics: none — the type lookup is the optional `node_type_opt`.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates a direct call, a function item
///   held in a binding, and the function pointer that stays invisible.
/// - witness: `tests::ui`
fn call_target(
    cx: &LateContext<'_>,
    callee: &Expr<'_>,
) -> Maybe<LocalDefId, call_resolution::Missing>
{
    if let ExprKind::Path(ref qpath) = callee.kind
        && let Res::Def(DefKind::Fn | DefKind::AssocFn, def_id) = cx.qpath_res(qpath, callee.hir_id)
    {
        return match def_id.as_local() {
            | Some(local) => Maybe::Present(local),
            | None => Maybe::Absent(call_resolution::Missing::NonLocal),
        };
    }
    let Some(callee_ty) = cx.typeck_results().node_type_opt(callee.hir_id)
    else {
        return Maybe::Absent(call_resolution::Missing::TypeUnavailable);
    };
    let rustc_ty::FnDef(def_id, _) = *callee_ty.kind()
    else {
        return Maybe::Absent(call_resolution::Missing::NotFunctionDefinition);
    };
    if !matches!(cx.tcx.def_kind(def_id), DefKind::Fn | DefKind::AssocFn) {
        return Maybe::Absent(call_resolution::Missing::NotFunctionOrMethod);
    }
    match def_id.as_local() {
        | Some(local) => Maybe::Present(local),
        | None => Maybe::Absent(call_resolution::Missing::NonLocal),
    }
}

/// Return every crate-local recursive strongly connected component, each sorted
/// by stable rustc path, the components themselves ordered by their first
/// member's path.
///
/// # Specification
/// - requires: `functions` holds every crate-local function the pass visited,
///   and `edges` maps callers to their crate-local callsites.
/// - ensures: a component is returned exactly when it has more than one member
///   or its single member calls itself; the result is a deterministic function
///   of the def-path strings in `functions`.
/// - provides: the denial set of [`crate::RECURSION_FORBIDDEN`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates direct self-recursion, mutual
///   recursion, and acyclic call chains; the component search itself is pinned
///   by the unit tests of [`tarjan_components`].
/// - witness: `tests::ui`
pub fn recursive_sccs(
    functions: &HashMap<LocalDefId, FunctionNode>,
    edges: &HashMap<LocalDefId, Vec<CallEdge>>,
) -> Vec<Vec<LocalDefId>>
{
    let nodes = sorted_function_ids(functions);
    let mut index_of: HashMap<LocalDefId, usize> = HashMap::with_capacity(nodes.len());
    for (position, def_id) in nodes.iter().enumerate() {
        index_of.insert(*def_id, position);
    }

    let node_count = nodes.len();
    let mut adjacency: Vec<Vec<Vertex>> = vec![Vec::new(); node_count];
    let mut self_loop: Vec<bool> = vec![false; node_count];
    for (position, def_id) in nodes.iter().enumerate() {
        let Some(call_edges) = edges.get(def_id)
        else {
            continue;
        };
        let mut successors: Vec<Vertex> = Vec::new();
        for edge in call_edges {
            let Some(callee) = index_of.get(&edge.callee).copied()
            else {
                continue;
            };
            if callee == position
                && let Some(slot) = self_loop.get_mut(position)
            {
                *slot = true;
            }
            successors.push(Vertex(callee));
        }
        successors.sort_unstable();
        successors.dedup();
        if let Some(slot) = adjacency.get_mut(position) {
            *slot = successors;
        }
    }

    let components = tarjan_components(&adjacency);
    let mut recursive: Vec<Vec<LocalDefId>> = Vec::new();
    for component in components {
        let is_recursive = component.len() > 1_usize
            || component
                .first()
                .and_then(|vertex| self_loop.get(vertex.0))
                .copied()
                .unwrap_or(false);
        if !is_recursive {
            continue;
        }
        let mut members: Vec<LocalDefId> = component
            .iter()
            .filter_map(|vertex| nodes.get(vertex.0).copied())
            .collect();
        members.sort_by_key(|def_id| functions.get(def_id).map(|node| node.path.as_str()));
        recursive.push(members);
    }
    recursive.sort_by_key(|component| {
        component
            .first()
            .and_then(|def_id| functions.get(def_id))
            .map(|node| node.path.clone())
    });
    recursive
}

/// Return crate-local function ids sorted by rustc's stable path string.
///
/// # Specification
/// - ensures: returns every key of the table, ordered by the stable path string
///   recorded with it, so the denial order is reproducible.
/// - panics: none.
fn sorted_function_ids(functions: &HashMap<LocalDefId, FunctionNode>) -> Vec<LocalDefId>
{
    let mut ids: Vec<_> = functions.keys().copied().collect();
    ids.sort_by_key(|def_id| functions.get(def_id).map(|node| node.path.as_str()));
    ids
}

/// Return whether any call edge inside `scc` passes caller-input-derived data.
///
/// # Specification
/// - requires: `scc` is one component returned by [`recursive_sccs`].
/// - ensures: answers affirmatively when some intra-component callsite has an
///   argument mentioning a local whose provenance reaches a parameter of the
///   calling function, and conservatively when an argument's HIR node can no
///   longer be resolved.
/// - provides: the refutation of a `- input recursion: none.` claim.
/// - panics: none.
pub fn scc_has_input_derived_recursive_call(
    cx: &LateContext<'_>,
    scc: &[LocalDefId],
    functions: &HashMap<LocalDefId, FunctionNode>,
    edges: &HashMap<LocalDefId, Vec<CallEdge>>,
) -> HasInputDerivedRecursiveCall
{
    let members: HashSet<_> = scc.iter().copied().collect();
    for caller in scc {
        let Some(node) = functions.get(caller)
        else {
            continue;
        };
        let derived = input_derived_bindings(cx, node);
        let Some(call_edges) = edges.get(caller)
        else {
            continue;
        };
        for edge in call_edges {
            if !members.contains(&edge.callee) {
                continue;
            }
            for arg in &edge.args {
                let Maybe::Present(expr) = expr_for_hir_id(cx, *arg)
                else {
                    return HasInputDerivedRecursiveCall(true);
                };
                if expr_contains_derived_binding(cx, &derived, expr).0 {
                    return HasInputDerivedRecursiveCall(true);
                }
            }
        }
    }
    HasInputDerivedRecursiveCall(false)
}

/// Return the final flow-insensitive set of locals derived from the function's
/// own parameters.
///
/// # Specification
/// - requires: `node` describes a crate-local function whose body root still
///   resolves to an expression node.
/// - ensures: the returned set contains every parameter binding and is closed
///   under `let`, `let`-expression, `match`, and assignment propagation.
/// - provides: the provenance set [`scc_has_input_derived_recursive_call`]
///   tests call arguments against.
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the fixed point is a loop over a monotone set.
/// - measure: the number of body bindings not yet in `derived`.
/// - boundedness: a pass repeats only when it inserted a binding, and the set
///   only grows within the body's finite binding count.
/// - input recursion: none.
fn input_derived_bindings(
    cx: &LateContext<'_>,
    node: &FunctionNode,
) -> HashSet<HirId>
{
    let mut derived: HashSet<_> = node.input_bindings.iter().copied().collect();
    let Maybe::Present(body) = expr_for_hir_id(cx, node.body_hir_id)
    else {
        return derived;
    };
    loop {
        let mut propagation = ProvenancePropagation {
            cx,
            derived: &mut derived,
            changed: false,
        };
        propagation.visit_expr(body);
        if !propagation.changed {
            break;
        }
    }
    derived
}

/// HIR visitor that grows the input-provenance set to a fixed point.
struct ProvenancePropagation<'derived, 'cx, 'tcx>
{
    /// The late lint context used for local resolution.
    cx: &'cx LateContext<'tcx>,
    /// The input-provenance set being grown to a fixed point.
    derived: &'derived mut HashSet<HirId>,
    /// Whether the current pass added any binding to `derived`.
    changed: bool,
}

#[expect(
    clippy::renamed_function_params,
    reason = "rustc declares the `Visitor` methods with single-letter parameter names (`ex`, `l`, \
              `p`); the implementation keeps descriptive names"
)]
impl<'tcx> Visitor<'tcx> for ProvenancePropagation<'_, '_, 'tcx>
{
    type MaybeTyCtxt = TyCtxt<'tcx>;
    type NestedFilter = nested_filter::OnlyBodies;

    /// Hand rustc's context to the walk.
    ///
    /// # Specification
    /// trivial.
    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt
    {
        self.cx.tcx
    }

    /// Propagate provenance across a `let` statement's initializer.
    ///
    /// # Specification
    /// - ensures: marks the statement's own bindings input-derived when its
    ///   initializer mentions a derived local, then walks its children.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is mutually recursive by
    ///   construction; see [`LocalCallCollector::visit_expr`].
    /// - measure: the height of the remaining HIR subtree below `local`.
    /// - boundedness: HIR trees are finite and depth-capped by the parser.
    /// - input recursion: none.
    fn visit_local(
        &mut self,
        local: &'tcx LetStmt<'tcx>,
    )
    {
        if let Some(init) = local.init
            && expr_contains_derived_binding(self.cx, self.derived, init).0
        {
            self.changed |= mark_pattern_bindings(local.pat, self.derived).0;
        }
        walk_local(self, local);
    }

    /// Propagate provenance across binding, scrutinee, and assignment forms.
    ///
    /// # Specification
    /// - ensures: marks the bindings of a `let` expression, of every match arm
    ///   whose scrutinee is derived, and of an assignment's target, then walks
    ///   the expression's children.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is mutually recursive by
    ///   construction; see [`LocalCallCollector::visit_expr`].
    /// - measure: the height of the remaining HIR subtree below `expr`.
    /// - boundedness: HIR trees are finite and depth-capped by the parser.
    /// - input recursion: none.
    fn visit_expr(
        &mut self,
        expr: &'tcx Expr<'_>,
    )
    {
        match expr.kind {
            | ExprKind::Let(let_expr) => {
                if expr_contains_derived_binding(self.cx, self.derived, let_expr.init).0 {
                    self.changed |= mark_pattern_bindings(let_expr.pat, self.derived).0;
                }
            },
            | ExprKind::Match(scrutinee, arms, _) => {
                if expr_contains_derived_binding(self.cx, self.derived, scrutinee).0 {
                    for arm in arms {
                        self.changed |= mark_pattern_bindings(arm.pat, self.derived).0;
                    }
                }
            },
            | ExprKind::Assign(target, value, _) => {
                if expr_contains_derived_binding(self.cx, self.derived, value).0 {
                    self.changed |= mark_local_references(target, self.cx, self.derived).0;
                }
            },
            | ExprKind::AssignOp(_, target, value)
                if expr_contains_derived_binding(self.cx, self.derived, target).0
                    || expr_contains_derived_binding(self.cx, self.derived, value).0 =>
            {
                self.changed |= mark_local_references(target, self.cx, self.derived).0;
            },
            | _ => {},
        }
        walk_expr(self, expr);
    }
}

/// Return all parameter pattern bindings in `body`.
///
/// # Specification
/// - ensures: returns every binding the body's parameter patterns introduce, in
///   parameter order, each id once.
/// - provides: the provenance seed of the input-recursion refutation.
/// - panics: none.
pub fn parameter_binding_ids(body: &Body<'_>) -> Vec<HirId>
{
    let mut bindings = Vec::new();
    for param in body.params {
        collect_pattern_bindings(param.pat, &mut bindings);
    }
    bindings
}

/// Mark every binding introduced by `pat` as input-derived.
///
/// # Specification
/// - ensures: adds every binding the pattern introduces to the set, and answers
///   affirmatively exactly when the set grew.
/// - panics: none.
fn mark_pattern_bindings(
    pat: &Pat<'_>,
    derived: &mut HashSet<HirId>,
) -> ProvenanceChanged
{
    let mut bindings = Vec::new();
    collect_pattern_bindings(pat, &mut bindings);
    let mut changed = false;
    for binding in bindings {
        changed |= derived.insert(binding);
    }
    ProvenanceChanged(changed)
}

/// Collect every binding introduced by `pat`.
///
/// # Specification
/// - ensures: appends every binding the pattern introduces, in visit order,
///   each id once.
/// - panics: none.
fn collect_pattern_bindings(
    pat: &Pat<'_>,
    bindings: &mut Vec<HirId>,
)
{
    let mut collector = PatternBindingCollector { bindings };
    collector.visit_pat(pat);
}

/// Pattern visitor that records local bindings.
#[repr(transparent)]
struct PatternBindingCollector<'bindings>
{
    /// The binding HIR ids recorded so far, in visit order.
    bindings: &'bindings mut Vec<HirId>,
}

#[expect(
    clippy::renamed_function_params,
    reason = "rustc declares the `Visitor` methods with single-letter parameter names (`ex`, `l`, \
              `p`); the implementation keeps descriptive names"
)]
impl<'tcx> Visitor<'tcx> for PatternBindingCollector<'_>
{
    /// Record a binding and descend into the pattern's children.
    ///
    /// # Specification
    /// - ensures: appends the pattern's own binding id when it has one and the
    ///   list does not, then walks the pattern's children.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is mutually recursive by
    ///   construction; see [`LocalCallCollector::visit_expr`].
    /// - measure: the height of the remaining HIR subtree below `pat`.
    /// - boundedness: HIR patterns are finite and depth-capped by the parser.
    /// - input recursion: none.
    fn visit_pat(
        &mut self,
        pat: &'tcx Pat<'_>,
    )
    {
        if let PatKind::Binding(_, hir_id, ..) = pat.kind
            && !self.bindings.contains(&hir_id)
        {
            self.bindings.push(hir_id);
        }
        walk_pat(self, pat);
    }
}

/// Return whether `expr` contains any reference to an input-derived local.
///
/// # Specification
/// - ensures: answers affirmatively exactly when some path in the expression
///   resolves to a local in the derived set.
/// - panics: none.
fn expr_contains_derived_binding<'tcx>(
    cx: &LateContext<'tcx>,
    derived: &HashSet<HirId>,
    expr: &'tcx Expr<'tcx>,
) -> ContainsDerivedBinding
{
    let mut visitor = DerivedBindingFinder {
        cx,
        derived,
        found: false,
    };
    visitor.visit_expr(expr);
    ContainsDerivedBinding(visitor.found)
}

/// Expression visitor that finds references to already-derived locals.
struct DerivedBindingFinder<'derived, 'cx, 'tcx>
{
    /// The late lint context used for local resolution.
    cx: &'cx LateContext<'tcx>,
    /// The current input-provenance set.
    derived: &'derived HashSet<HirId>,
    /// Whether a reference to a derived local has been found.
    found: bool,
}

#[expect(
    clippy::renamed_function_params,
    reason = "rustc declares the `Visitor` methods with single-letter parameter names (`ex`, `l`, \
              `p`); the implementation keeps descriptive names"
)]
impl<'tcx> Visitor<'tcx> for DerivedBindingFinder<'_, '_, 'tcx>
{
    type MaybeTyCtxt = TyCtxt<'tcx>;
    type NestedFilter = nested_filter::OnlyBodies;

    /// Hand rustc's context to the walk.
    ///
    /// # Specification
    /// trivial.
    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt
    {
        self.cx.tcx
    }

    /// Stop at the first derived local, otherwise descend.
    ///
    /// # Specification
    /// - ensures: sets the found flag on the first path resolving to a derived
    ///   local and stops descending once it is set.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is mutually recursive by
    ///   construction; see [`LocalCallCollector::visit_expr`].
    /// - measure: the height of the remaining HIR subtree below `expr`.
    /// - boundedness: HIR trees are finite and depth-capped by the parser.
    /// - input recursion: none.
    fn visit_expr(
        &mut self,
        expr: &'tcx Expr<'_>,
    )
    {
        if self.found {
            return;
        }
        if let ExprKind::Path(ref qpath) = expr.kind
            && let Res::Local(hir_id) = self.cx.qpath_res(qpath, expr.hir_id)
            && self.derived.contains(&hir_id)
        {
            self.found = true;
            return;
        }
        walk_expr(self, expr);
    }
}

/// Mark every local reference in `expr`; used conservatively for assignments.
///
/// # Specification
/// - ensures: adds every local the expression references to the set, and
///   answers affirmatively exactly when the set grew. Marking every reference
///   rather than the assigned place alone is the conservative direction for a
///   ban with an explicit escape hatch.
/// - panics: none.
fn mark_local_references<'tcx>(
    expr: &'tcx Expr<'tcx>,
    cx: &LateContext<'tcx>,
    derived: &mut HashSet<HirId>,
) -> ProvenanceChanged
{
    let mut collector = LocalReferenceCollector {
        cx,
        locals: Vec::new(),
    };
    collector.visit_expr(expr);
    let mut changed = false;
    for local in collector.locals {
        changed |= derived.insert(local);
    }
    ProvenanceChanged(changed)
}

/// Expression visitor that records every referenced local binding.
struct LocalReferenceCollector<'cx, 'tcx>
{
    /// The late lint context used for local resolution.
    cx: &'cx LateContext<'tcx>,
    /// The referenced local binding HIR ids recorded so far, in visit order.
    locals: Vec<HirId>,
}

#[expect(
    clippy::renamed_function_params,
    reason = "rustc declares the `Visitor` methods with single-letter parameter names (`ex`, `l`, \
              `p`); the implementation keeps descriptive names"
)]
impl<'tcx> Visitor<'tcx> for LocalReferenceCollector<'_, 'tcx>
{
    type MaybeTyCtxt = TyCtxt<'tcx>;
    type NestedFilter = nested_filter::OnlyBodies;

    /// Hand rustc's context to the walk.
    ///
    /// # Specification
    /// trivial.
    fn maybe_tcx(&mut self) -> Self::MaybeTyCtxt
    {
        self.cx.tcx
    }

    /// Record a referenced local and descend into the expression's children.
    ///
    /// # Specification
    /// - ensures: appends every local a path in the expression resolves to,
    ///   each id once, then walks the expression's children.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: rustc's `intravisit` walk is mutually recursive by
    ///   construction; see [`LocalCallCollector::visit_expr`].
    /// - measure: the height of the remaining HIR subtree below `expr`.
    /// - boundedness: HIR trees are finite and depth-capped by the parser.
    /// - input recursion: none.
    fn visit_expr(
        &mut self,
        expr: &'tcx Expr<'_>,
    )
    {
        if let ExprKind::Path(ref qpath) = expr.kind
            && let Res::Local(hir_id) = self.cx.qpath_res(qpath, expr.hir_id)
            && !self.locals.contains(&hir_id)
        {
            self.locals.push(hir_id);
        }
        walk_expr(self, expr);
    }
}

/// Return an expression by HIR id when the id still names an expression node.
///
/// # Specification
/// - ensures: returns the expression exactly when the id still names an
///   expression node, and nothing when the node is of another kind.
/// - provides: `expression_lookup::Missing::NotExpression` identifies the HIR
///   kind mismatch, preserving the caller's conservative provenance decision.
/// - panics: none.
fn expr_for_hir_id<'tcx>(
    cx: &LateContext<'tcx>,
    hir_id: HirId,
) -> Maybe<&'tcx Expr<'tcx>, expression_lookup::Missing>
{
    match cx.tcx.hir_node(hir_id) {
        | Node::Expr(expr) => Maybe::Present(expr),
        | _ => Maybe::Absent(expression_lookup::Missing::NotExpression),
    }
}
