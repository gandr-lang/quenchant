//! Recursive ownership over local type definitions, not pointer-name
//! heuristics.
//!
//! A definition is rejected when an owned field path returns to that
//! definition. `Vec<Self>` can close such a path without spelling a pointer; a
//! boxed object that returns only through an arena index does not. Rust's
//! finite-size rule already forces an indirection somewhere in a genuine
//! ownership cycle.
//!
//! Owned positions are those whose contents are dropped with the enclosing
//! value. The walk follows arrays, slices, pattern types, tuples, and owned ADT
//! parameters. It stops at references, raw pointers, function values, closures,
//! trait objects, and unnormalized projections. `PhantomData`, weak references,
//! and explicitly justified foreign types supply named non-owning boundaries.
//!
//! Local parameter ownership is a least fixed point over fields. This keeps a
//! typed arena identifier from becoming an owning edge merely because its
//! phantom parameter names the enclosing type.
//!
//! Foreign representations cannot be inspected as if their raw storage pointers
//! described their ownership semantics: doing so would hide ownership in
//! containers such as `Vec`. Unknown foreign parameters are conservatively
//! owning; the justified allowlist is the channel for correcting that model.
//!
//! Diagnostics name members of recursive strongly connected components. A type
//! that contains a different cyclic type is not automatically in that cycle.

use alloc::collections::VecDeque;
use std::collections::HashMap;
use std::collections::HashSet;

use rustc_hir::HirId;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LocalDefId;
use rustc_lint::LateContext;
use rustc_middle::ty as rustc_ty;
use rustc_span::Span;
use rustc_span::symbol::sym;

use crate::graph::tarjan_components;
use crate::semantic::NonOwningAdt;
use crate::semantic::OwnedParameter;
use crate::semantic::OwnedParametersChanged;
use crate::semantic::ParameterIndex;
use crate::semantic::TypePath;
use crate::semantic::Vertex;
use crate::signature::normalize_middle_ty;

/// Type identity and diagnostic ownership retained for whole-crate cycle
/// analysis.
pub struct AdtNode
{
    /// Item identity preserves its local lint-level policy.
    pub hir_id: HirId,
    /// The authored type name anchors the diagnostic.
    pub span: Span,
    /// Stable type spelling determines cycle rendering and diagnostic order.
    pub path: String,
}

/// A denied type remains paired with the ownership path explaining its cycle.
pub struct OwningCycle
{
    /// Local type at which this cycle is reported.
    pub adt: LocalDefId,
    /// Field-qualified ownership hops close back on the starting type.
    pub cycle: String,
}

/// Justified external-generic boundaries remove otherwise conservative
/// ownership edges.
///
/// Workspace `dylint.toml` entries pair definition paths with evidence; the
/// crate README defines the schema. Entries lacking justification remain owning
/// and produce configuration diagnostics.
#[derive(Clone, Default)]
pub struct NonOwningAllowList
{
    /// External definitions whose arguments are justified as non-owning.
    paths: Vec<String>,
    /// Incomplete entries remain visible as configuration defects.
    defects: Vec<String>,
}

impl NonOwningAllowList
{
    /// Only an admitted exact definition path removes the conservative owning
    /// interpretation.
    ///
    /// # Specification
    /// - ensures: answers affirmatively exactly when a justified entry names
    ///   that def path; a refused entry admits nothing.
    /// - panics: none.
    fn admits(
        &self,
        path: TypePath<'_>,
    ) -> NonOwningAdt
    {
        NonOwningAdt(self.paths.iter().any(|entry| entry == path.0))
    }

    /// Rejected configuration entries retain the reason admission failed.
    ///
    /// # Specification
    /// trivial.
    pub fn defects(&self) -> &[String]
    {
        &self.defects
    }
}

/// Configuration key selecting the external ownership-boundary declarations.
const ALLOW_LIST_KEY: &str = "non_owning_generics";

/// Incomplete ownership declarations receive the required evidence shape.
const UNJUSTIFIED_ENTRY: &str = concat!(
    "each `non_owning_generics` entry must state `path` and a non-empty `justification`; the ",
    "entry is refused, so the type stays conservatively owning",
);

/// Configuration evidence determines which external ownership boundaries are
/// admitted.
///
/// # Specification
/// - requires: `dylint_linting::init_config` has already run, which
///   [`crate::register_lints`] guarantees by calling it first.
/// - ensures: an entry reaches the returned list exactly when it states both a
///   `path` and a non-empty `justification`; every other entry is reported as a
///   defect and admits nothing.
/// - provides: the override of the conservative external-generic rule.
/// - fails: an unreadable or malformed configuration yields an empty list, so
///   the gate falls back to denying rather than to admitting.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates a conservative denial with no
///   configuration, the same type admitted by a justified entry, and an entry
///   whose justification is missing.
/// - witness: `tests::ui`
/// - witness: `tests::ui_config`
pub fn non_owning_allow_list() -> NonOwningAllowList
{
    let mut list = NonOwningAllowList::default();
    let Ok(Some(config)) = dylint_linting::config_toml(env!("CARGO_PKG_NAME"))
    else {
        return list;
    };
    let Some(entries) = config
        .get(ALLOW_LIST_KEY)
        .and_then(|value| value.as_array())
    else {
        return list;
    };
    for entry in entries {
        let path = entry.get("path").and_then(|value| value.as_str());
        let justification = entry.get("justification").and_then(|value| value.as_str());
        match (path, justification) {
            | (Some(path), Some(justification)) if !justification.trim().is_empty() => {
                list.paths.push(path.to_owned());
            },
            | (path, _) => {
                list.defects.push(format!(
                    "{UNJUSTIFIED_ENTRY} (entry: `{}`)",
                    path.unwrap_or("<no path>")
                ));
            },
        }
    }
    list
}

/// Owning cycles produce deterministic type-local denials.
///
/// # Specification
/// - requires: `adts` holds every crate-local ADT the pass visited.
/// - ensures: an ADT is returned exactly when it lies on a cycle of the
///   ownership-reachability graph — a strongly connected component of more than
///   one member, or a member owning itself directly; the result and each
///   rendered cycle are deterministic functions of the def-path strings in
///   `adts`.
/// - provides: the denial set of [`crate::RECURSIVE_OWNED_POINTER`].
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the graph is built by worklist loops and searched by
///   the iterative component pass and a breadth-first queue.
/// - measure: the number of ADTs whose edges are still unbuilt.
/// - boundedness: the ADT set is the finite set of crate-local definitions.
/// - input recursion: none.
///
/// # Adequacy
/// - hypothesis: L3 — the UI matrix separates the direct cycle, the mutual
///   cycle, each owning container, and the acyclic shapes that must pass (arena
///   indices, references, boxes of non-recursive elements, and the arena table
///   that merely holds cycle members).
/// - witness: `tests::ui`
/// - witness: `ownership::tests::a_container_of_cycle_members_is_not_in_the_cycle`
/// - witness: `ownership::tests::a_self_edge_renders_one_hop`
/// - witness: `ownership::tests::a_mutual_cycle_renders_both_hops`
pub fn owning_cycles(
    cx: &LateContext<'_>,
    adts: &HashMap<LocalDefId, AdtNode>,
    allow_list: &NonOwningAllowList,
) -> Vec<OwningCycle>
{
    let nodes = sorted_adt_ids(adts);
    let owned = owned_parameters(cx, &nodes, allow_list);
    let walk = OwnershipWalk {
        cx,
        allow_list,
        owned_parameters: &owned,
    };

    let mut index_of: HashMap<LocalDefId, Vertex> = HashMap::with_capacity(nodes.len());
    for (position, def_id) in nodes.iter().enumerate() {
        index_of.insert(*def_id, Vertex(position));
    }
    let mut names: Vec<String> = Vec::with_capacity(nodes.len());
    for def_id in &nodes {
        names.push(adts.get(def_id).map_or_else(String::new, |node| {
            node.path.rsplit("::").next().unwrap_or("").to_owned()
        }));
    }

    let mut adjacency: Vec<Vec<Vertex>> = vec![Vec::new(); nodes.len()];
    let mut labels: HashMap<(Vertex, Vertex), String> = HashMap::new();
    for (position, def_id) in nodes.iter().enumerate() {
        let source = Vertex(position);
        let mut successors: Vec<Vertex> = Vec::new();
        for (label, field_ty) in owned_field_types(cx, *def_id) {
            for reached in walk.reach(field_ty).adts {
                let Some(target) = index_of.get(&reached).copied()
                else {
                    continue;
                };
                successors.push(target);
                labels
                    .entry((source, target))
                    .or_insert_with(|| label.clone());
            }
        }
        successors.sort_unstable();
        successors.dedup();
        if let Some(slot) = adjacency.get_mut(position) {
            *slot = successors;
        }
    }

    let mut denials: Vec<OwningCycle> = Vec::new();
    for component in tarjan_components(&adjacency) {
        let members: HashSet<Vertex> = component.iter().copied().collect();
        let is_cyclic = component.len() > 1_usize
            || component.first().is_some_and(|vertex| {
                adjacency
                    .get(vertex.0)
                    .is_some_and(|successors| successors.contains(vertex))
            });
        if !is_cyclic {
            continue;
        }
        for vertex in component {
            let Some(def_id) = nodes.get(vertex.0).copied()
            else {
                continue;
            };
            denials.push(OwningCycle {
                adt: def_id,
                cycle: render_cycle(vertex, &adjacency, &members, &labels, &names),
            });
        }
    }
    denials.sort_by_key(|denial| adts.get(&denial.adt).map(|node| node.path.clone()));
    denials
}

/// Stable definition paths determine type traversal order.
///
/// # Specification
/// - ensures: returns every key of the table, ordered by the stable path string
///   recorded with it, so the denial order is reproducible.
/// - panics: none.
fn sorted_adt_ids(adts: &HashMap<LocalDefId, AdtNode>) -> Vec<LocalDefId>
{
    let mut ids: Vec<_> = adts.keys().copied().collect();
    ids.sort_by_key(|def_id| adts.get(def_id).map(|node| node.path.as_str()));
    ids
}

/// Field identity remains paired with its identity-instantiated compiler type.
///
/// Labels preserve source ownership: enum fields include the variant, struct
/// and union fields use their name, and tuple fields use their position.
///
/// # Specification
/// - requires: `def_id` identifies a crate-local ADT.
/// - ensures: returns one entry per field of every variant, in declaration
///   order, each type instantiated with the ADT's own parameters.
/// - panics: none.
fn owned_field_types<'tcx>(
    cx: &LateContext<'tcx>,
    def_id: LocalDefId,
) -> Vec<(String, rustc_ty::Ty<'tcx>)>
{
    let adt = cx.tcx.adt_def(def_id);
    let args = rustc_ty::GenericArgs::identity_for_item(cx.tcx, def_id.to_def_id());
    let mut fields = Vec::new();
    for variant in adt.variants() {
        for field in &variant.fields {
            let label = if adt.is_enum() {
                format!("{}::{}", variant.name, field.name)
            }
            else {
                field.name.to_string()
            };
            // The pinned compiler exposes this field type before normalization. `reach`
            // normalizes every popped type, so classification still uses the normalized
            // view.
            fields.push((label, field.ty(cx.tcx, args).skip_norm_wip()));
        }
    }
    fields
}

/// Owned generic positions are inferred by a monotone fixed point over local
/// fields.
///
/// # Specification
/// - requires: `nodes` lists every crate-local ADT.
/// - ensures: the returned flag is set exactly when the parameter is reachable
///   from some field of the ADT through owned positions only, taking every
///   parameter of an ADT outside the crate as owned.
/// - provides: the instantiation rule the edge walk descends generic arguments
///   by.
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the fixed point is a loop over a monotone set of
///   flags.
/// - measure: the number of parameter flags still unset.
/// - boundedness: a pass repeats only when it set a flag, and the flags are a
///   finite set that only grows.
/// - input recursion: none.
fn owned_parameters(
    cx: &LateContext<'_>,
    nodes: &[LocalDefId],
    allow_list: &NonOwningAllowList,
) -> HashMap<DefId, Vec<OwnedParameter>>
{
    let mut owned: HashMap<DefId, Vec<OwnedParameter>> = HashMap::with_capacity(nodes.len());
    for def_id in nodes {
        let count = cx.tcx.generics_of(def_id.to_def_id()).count();
        owned.insert(def_id.to_def_id(), vec![OwnedParameter(false); count]);
    }
    // economy: each round revisits every local field. Remaining false flags bound
    // the number of rounds; a dependency-driven worklist is the upgrade if repeated
    // scans dominate a profile.
    loop {
        let mut marks: Vec<(DefId, Vec<ParameterIndex>)> = Vec::with_capacity(nodes.len());
        {
            let walk = OwnershipWalk {
                cx,
                allow_list,
                owned_parameters: &owned,
            };
            for def_id in nodes {
                let mut marked: Vec<ParameterIndex> = Vec::new();
                for (_, field_ty) in owned_field_types(cx, *def_id) {
                    marked.extend(walk.reach(field_ty).parameters);
                }
                marks.push((def_id.to_def_id(), marked));
            }
        }
        let mut changed = OwnedParametersChanged(false);
        for (def_id, marked) in marks {
            let Some(flags) = owned.get_mut(&def_id)
            else {
                continue;
            };
            for index in marked {
                if let Some(slot) = flags.get_mut(index.0)
                    && !slot.0
                {
                    *slot = OwnedParameter(true);
                    changed = OwnedParametersChanged(true);
                }
            }
        }
        if !changed.0 {
            break;
        }
    }
    owned
}

/// Ownership reachability retains local type destinations and generic positions
/// separately.
#[derive(Default)]
struct Reached
{
    /// Local type destinations preserve encounter order.
    adts: Vec<LocalDefId>,
    /// Generic positions reached through ownership in the walked declaration.
    parameters: Vec<ParameterIndex>,
}

/// Normalized compiler types determine ownership reachability.
struct OwnershipWalk<'walk, 'tcx>
{
    /// Compiler context supplies normalization and definition identity.
    cx: &'walk LateContext<'tcx>,
    /// Justified external boundaries stop ownership propagation through
    /// arguments.
    allow_list: &'walk NonOwningAllowList,
    /// Current fixed-point approximation of local generic ownership.
    owned_parameters: &'walk HashMap<DefId, Vec<OwnedParameter>>,
}

impl<'tcx> OwnershipWalk<'_, 'tcx>
{
    /// Only owned positions contribute type or parameter reachability.
    ///
    /// # Specification
    /// - requires: `root` is a field type instantiated with its ADT's identity
    ///   arguments.
    /// - ensures: a crate-local ADT is reported exactly when dropping a value
    ///   of `root` drops a value of that ADT, under the owned-position rules in
    ///   this module's documentation.
    /// - provides: one ADT's outgoing ownership edges, and the parameter
    ///   occurrences the fixed point consumes.
    /// - panics: none.
    ///
    /// # Termination
    /// - reason: no recursion; the traversal is a loop over an explicit
    ///   worklist.
    /// - measure: the total size of the type trees still pending.
    /// - boundedness: every push is a strict subterm of the popped type, type
    ///   trees are finite, and a type already visited is never pushed again.
    /// - input recursion: none.
    fn reach(
        &self,
        root: rustc_ty::Ty<'tcx>,
    ) -> Reached
    {
        let mut pending = vec![root];
        let mut seen: HashSet<rustc_ty::Ty<'tcx>> = HashSet::new();
        let mut reached = Reached::default();
        while let Some(ty) = pending.pop() {
            let ty = normalize_middle_ty(self.cx, ty);
            if !seen.insert(ty) {
                continue;
            }
            match *ty.kind() {
                | rustc_ty::Adt(adt, args) => {
                    if self.is_non_owning(adt).0 {
                        continue;
                    }
                    if let Some(local) = adt.did().as_local() {
                        reached.adts.push(local);
                    }
                    for (position, arg) in args.iter().enumerate() {
                        let index = ParameterIndex(position);
                        if let Some(inner) = arg.as_type()
                            && self.is_owned_parameter(adt.did(), index).0
                        {
                            pending.push(inner);
                        }
                    }
                },
                | rustc_ty::Array(inner, _) | rustc_ty::Slice(inner) | rustc_ty::Pat(inner, _) => {
                    pending.push(inner);
                },
                | rustc_ty::Tuple(types) => pending.extend(types.iter()),
                | rustc_ty::Param(param) => {
                    reached.parameters.push(ParameterIndex(
                        usize::try_from(param.index).unwrap_or(usize::MAX),
                    ));
                },
                | _ => {},
            }
        }
        reached
    }

    /// A recognized non-owning ADT stops traversal through all of its
    /// arguments.
    ///
    /// # Specification
    /// - ensures: answers affirmatively for `PhantomData`, for either `Weak`,
    ///   and for an external ADT a justified allow-list entry names; a
    ///   crate-local ADT answers negatively, since its own parameters are
    ///   analysed instead.
    /// - panics: none.
    fn is_non_owning(
        &self,
        adt: rustc_ty::AdtDef<'tcx>,
    ) -> NonOwningAdt
    {
        if adt.is_phantom_data() {
            return NonOwningAdt(true);
        }
        let def_id = adt.did();
        if matches!(
            self.cx.tcx.get_diagnostic_name(def_id),
            Some(sym::RcWeak | sym::ArcWeak)
        ) {
            return NonOwningAdt(true);
        }
        if def_id.is_local() || self.allow_list.paths.is_empty() {
            return NonOwningAdt(false);
        }
        // economy: a nonempty allow-list requires one rendered definition path per
        // reached external ADT. Resolving the list to definition ids once per crate
        // would remove that allocation if long lists become costly.
        self.allow_list
            .admits(TypePath::from(self.cx.tcx.def_path_str(def_id).as_str()))
    }

    /// Generic-position ownership uses local evidence and conservative external
    /// assumptions.
    ///
    /// Local definitions supply computed flags. External definitions have no
    /// such field analysis, so every argument position is treated as owning.
    ///
    /// # Specification
    /// - ensures: answers from the computed flags for a crate-local ADT, and
    ///   affirmatively for every parameter of an ADT with no flags, including
    ///   an index past the recorded ones.
    /// - panics: none.
    fn is_owned_parameter(
        &self,
        def_id: DefId,
        index: ParameterIndex,
    ) -> OwnedParameter
    {
        self.owned_parameters
            .get(&def_id)
            .map_or(OwnedParameter(true), |flags| {
                flags.get(index.0).copied().unwrap_or(OwnedParameter(true))
            })
    }
}

/// A shortest in-component cycle supplies a bounded diagnostic explanation.
///
/// # Specification
/// - requires: `start` lies on a cycle of `adjacency`, and `members` is the
///   strongly connected component containing it.
/// - ensures: the rendered path alternates the field carrying each hop with the
///   type it reaches, and closes on the type it started from; a start with no
///   cycle inside `members` renders as its own name alone.
/// - provides: the cycle text of the diagnostic.
/// - panics: none.
///
/// # Termination
/// - reason: no recursion; the search is a queue loop and the reconstruction
///   walks a predecessor chain of already-visited vertices.
/// - measure: the number of component members not yet enqueued, then the length
///   of the predecessor chain.
/// - boundedness: a vertex is enqueued only once, and each predecessor step
///   moves to the vertex that enqueued the current one, so the chain is the
///   finite discovery order.
/// - input recursion: none.
fn render_cycle(
    start: Vertex,
    adjacency: &[Vec<Vertex>],
    members: &HashSet<Vertex>,
    labels: &HashMap<(Vertex, Vertex), String>,
    names: &[String],
) -> String
{
    let name = |vertex: Vertex| names.get(vertex.0).cloned().unwrap_or_default();
    let mut previous: HashMap<Vertex, Vertex> = HashMap::new();
    let mut queue: VecDeque<Vertex> = VecDeque::new();
    let mut closing: Option<Vertex> = None;
    queue.push_back(start);
    'search: while let Some(vertex) = queue.pop_front() {
        let Some(successors) = adjacency.get(vertex.0)
        else {
            continue;
        };
        for successor in successors {
            if !members.contains(successor) {
                continue;
            }
            if *successor == start {
                closing = Some(vertex);
                break 'search;
            }
            if !previous.contains_key(successor) {
                previous.insert(*successor, vertex);
                queue.push_back(*successor);
            }
        }
    }
    let Some(closing) = closing
    else {
        return name(start);
    };

    let mut chain: Vec<Vertex> = vec![closing];
    let mut cursor = closing;
    while cursor != start {
        let Some(parent) = previous.get(&cursor).copied()
        else {
            break;
        };
        chain.push(parent);
        cursor = parent;
    }
    chain.reverse();

    let mut rendered = String::new();
    for (position, vertex) in chain.iter().enumerate() {
        let next = chain.get(position.saturating_add(1_usize)).copied();
        let target = next.unwrap_or(start);
        let label = labels
            .get(&(*vertex, target))
            .cloned()
            .unwrap_or_else(|| "?".to_owned());
        rendered.push('`');
        rendered.push_str(&name(*vertex));
        rendered.push_str("::");
        rendered.push_str(&label);
        rendered.push_str("` → ");
    }
    rendered.push('`');
    rendered.push_str(&name(start));
    rendered.push('`');
    rendered
}

#[cfg(test)]
mod tests
{
    use std::collections::HashMap;
    use std::collections::HashSet;

    use super::render_cycle;
    use crate::graph::tarjan_components;
    use crate::semantic::FieldLabel;
    use crate::semantic::Vertex;

    /// Fixture edges retain the field labels used by cycle rendering.
    ///
    /// # Specification
    /// trivial.
    fn labels(entries: &[(Vertex, Vertex, FieldLabel<'_>)]) -> HashMap<(Vertex, Vertex), String>
    {
        entries
            .iter()
            .map(|&(source, target, field)| ((source, target), field.0.to_owned()))
            .collect()
    }

    /// Component membership isolates the cycle containing the selected fixture
    /// vertex.
    ///
    /// # Specification
    /// trivial.
    fn component_of(
        adjacency: &[Vec<Vertex>],
        vertex: Vertex,
    ) -> HashSet<Vertex>
    {
        tarjan_components(adjacency)
            .into_iter()
            .find(|component| component.contains(&vertex))
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    #[test]
    fn a_container_of_cycle_members_is_not_in_the_cycle()
    {
        // Vertex 0 holds a row in the 1–2 cycle but has no return edge. The arena
        // remains outside the recursive component even though it reaches a cyclic row.
        let adjacency = vec![vec![Vertex(1)], vec![Vertex(2)], vec![Vertex(1)]];
        let mut components = tarjan_components(&adjacency);
        for component in &mut components {
            component.sort_unstable();
        }
        components.sort();
        assert_eq!(
            components,
            vec![vec![Vertex(0)], vec![Vertex(1), Vertex(2)]],
            "the container that merely holds cycle members forms no cycle of its own"
        );
    }

    #[test]
    fn a_self_edge_renders_one_hop()
    {
        let adjacency = vec![vec![Vertex(0)]];
        let names = vec!["Node".to_owned()];
        assert_eq!(
            render_cycle(
                Vertex(0),
                &adjacency,
                &component_of(&adjacency, Vertex(0)),
                &labels(&[(Vertex(0), Vertex(0), FieldLabel("child"))]),
                &names,
            ),
            "`Node::child` → `Node`",
            "a direct self-edge names the field that closes it"
        );
    }

    #[test]
    fn a_mutual_cycle_renders_both_hops()
    {
        let adjacency = vec![vec![Vertex(1)], vec![Vertex(0)]];
        let names = vec!["Left".to_owned(), "Right".to_owned()];
        let table = labels(&[
            (Vertex(0), Vertex(1), FieldLabel("right")),
            (Vertex(1), Vertex(0), FieldLabel("left")),
        ]);
        assert_eq!(
            render_cycle(
                Vertex(0),
                &adjacency,
                &component_of(&adjacency, Vertex(0)),
                &table,
                &names,
            ),
            "`Left::right` → `Right::left` → `Left`",
            "a mutual cycle names every field on the way back"
        );
    }
}
