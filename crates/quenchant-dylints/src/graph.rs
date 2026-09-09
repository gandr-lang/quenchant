//! One iterative component partition for two different semantic graphs.
//!
//! Call analysis supplies function vertices; ownership analysis supplies local
//! ADT vertices. Both retain their own meanings and diagnostic labels outside
//! this position-indexed traversal. A component here is graph evidence, not a
//! claim that the two analyses detect the same runtime behavior.
//!
//! Explicit suspended frames preserve Tarjan's parent/child bookkeeping without
//! recursion over a source-controlled graph depth.

use crate::semantic::Vertex;

/// Explicit continuation state for one active Tarjan vertex.
struct TarjanFrame
{
    /// Vertex whose unfinished traversal this frame resumes.
    node: usize,
    /// Cursor preserves progress through that vertex's successors.
    next_edge: usize,
}

/// Tarjan components are computed with heap-resident continuations instead of
/// recursive calls.
///
/// # Specification
/// - requires: every successor in `adjacency` is a valid vertex position.
/// - ensures: each vertex appears in exactly one returned component.
/// - provides: the component partition both cycle gates filter.
/// - panics: none — every vertex lookup is a checked `get`.
///
/// # Termination
/// - reason: no recursion; the traversal is a loop over an explicit stack.
/// - measure: the number of unvisited vertices plus unconsumed successor
///   entries.
/// - boundedness: a vertex is pushed only when its order slot is still empty,
///   and the slot is filled before the push, so each vertex is pushed once and
///   each successor entry is consumed once.
/// - input recursion: none.
///
/// # Adequacy
/// - hypothesis: L3 — the unit tests below pin the two decisive paths of the
///   component search: the `on_stack` guard that keeps a cross edge into a
///   finished component from swallowing its source vertex, and the
///   parent-lowlink propagation that merges a cycle closed through a later
///   child subtree.
/// - witness: `graph::tests::self_loop_is_recursive`
/// - witness: `graph::tests::two_cycle_is_one_component`
/// - witness: `graph::tests::acyclic_chain_has_no_component`
/// - witness: `graph::tests::nested_cycles_separate_by_reachability`
/// - witness: `graph::tests::cross_edge_into_a_finished_component_does_not_merge_it`
/// - witness: `graph::tests::a_later_child_subtree_lowers_the_parent_lowlink`
pub fn tarjan_components(adjacency: &[Vec<Vertex>]) -> Vec<Vec<Vertex>>
{
    let node_count = adjacency.len();
    let mut order: Vec<Option<usize>> = vec![None; node_count];
    let mut low: Vec<usize> = vec![0_usize; node_count];
    let mut on_stack: Vec<bool> = vec![false; node_count];
    let mut pending: Vec<usize> = Vec::new();
    let mut frames: Vec<TarjanFrame> = Vec::new();
    let mut next_order = 0_usize;
    let mut components: Vec<Vec<Vertex>> = Vec::new();

    for root in 0 .. node_count {
        if order.get(root).copied().flatten().is_some() {
            continue;
        }
        if let Some(slot) = order.get_mut(root) {
            *slot = Some(next_order);
        }
        if let Some(slot) = low.get_mut(root) {
            *slot = next_order;
        }
        if let Some(slot) = on_stack.get_mut(root) {
            *slot = true;
        }
        next_order = next_order.saturating_add(1_usize);
        pending.push(root);
        frames.push(TarjanFrame {
            node: root,
            next_edge: 0_usize,
        });

        while let Some(frame) = frames.last() {
            let current = frame.node;
            let edge_index = frame.next_edge;
            let successor = adjacency
                .get(current)
                .and_then(|list| list.get(edge_index))
                .map(|vertex| vertex.0);
            let Some(next) = successor
            else {
                frames.pop();
                let current_low = low.get(current).copied().unwrap_or(0_usize);
                if let Some(parent) = frames.last() {
                    let parent_node = parent.node;
                    let updated = low
                        .get(parent_node)
                        .copied()
                        .unwrap_or(0_usize)
                        .min(current_low);
                    if let Some(slot) = low.get_mut(parent_node) {
                        *slot = updated;
                    }
                }
                if order.get(current).copied().flatten() == Some(current_low) {
                    let mut component: Vec<Vertex> = Vec::new();
                    while let Some(top) = pending.pop() {
                        if let Some(slot) = on_stack.get_mut(top) {
                            *slot = false;
                        }
                        component.push(Vertex(top));
                        if top == current {
                            break;
                        }
                    }
                    components.push(component);
                }
                continue;
            };
            if let Some(frame) = frames.last_mut() {
                frame.next_edge = edge_index.saturating_add(1_usize);
            }
            match order.get(next).copied().flatten() {
                | None => {
                    if let Some(slot) = order.get_mut(next) {
                        *slot = Some(next_order);
                    }
                    if let Some(slot) = low.get_mut(next) {
                        *slot = next_order;
                    }
                    if let Some(slot) = on_stack.get_mut(next) {
                        *slot = true;
                    }
                    next_order = next_order.saturating_add(1_usize);
                    pending.push(next);
                    frames.push(TarjanFrame {
                        node: next,
                        next_edge: 0_usize,
                    });
                },
                | Some(next_position) => {
                    if on_stack.get(next).copied().unwrap_or(false) {
                        let updated = low
                            .get(current)
                            .copied()
                            .unwrap_or(0_usize)
                            .min(next_position);
                        if let Some(slot) = low.get_mut(current) {
                            *slot = updated;
                        }
                    }
                },
            }
        }
    }
    components
}

#[cfg(test)]
mod tests
{
    use super::tarjan_components;
    use crate::semantic::Vertex;

    /// Canonicalized membership lets tests ignore incidental discovery order.
    ///
    /// # Specification
    /// trivial.
    fn normalized(adjacency: &[Vec<Vertex>]) -> Vec<Vec<Vertex>>
    {
        let mut components = tarjan_components(adjacency);
        for component in &mut components {
            component.sort_unstable();
        }
        components.sort();
        components
    }

    #[test]
    fn self_loop_is_recursive()
    {
        let adjacency = vec![vec![Vertex(0)]];
        assert_eq!(
            normalized(&adjacency),
            vec![vec![Vertex(0)]],
            "a self-loop is one vertex"
        );
    }

    #[test]
    fn two_cycle_is_one_component()
    {
        let adjacency = vec![vec![Vertex(1)], vec![Vertex(0)]];
        assert_eq!(
            normalized(&adjacency),
            vec![vec![Vertex(0), Vertex(1)]],
            "a mutual pair forms a single component"
        );
    }

    #[test]
    fn acyclic_chain_has_no_component()
    {
        let adjacency = vec![vec![Vertex(1)], vec![Vertex(2)], Vec::new()];
        assert_eq!(
            normalized(&adjacency),
            vec![vec![Vertex(0)], vec![Vertex(1)], vec![Vertex(2)]],
            "an acyclic chain is three singletons and none absorbs another"
        );
    }

    #[test]
    fn nested_cycles_separate_by_reachability()
    {
        // The 1–2 cycle reaches terminal vertex 3 without making 3 a cycle member.
        let adjacency = vec![
            vec![Vertex(1)],
            vec![Vertex(2)],
            vec![Vertex(1), Vertex(3)],
            Vec::new(),
        ];
        assert_eq!(
            normalized(&adjacency),
            vec![vec![Vertex(0)], vec![Vertex(1), Vertex(2)], vec![Vertex(3)]],
            "only the mutually reachable pair merges"
        );
    }

    #[test]
    fn cross_edge_into_a_finished_component_does_not_merge_it()
    {
        // Exploring 0's first successor completes component {1, 2}; the second
        // successor reaches that already-finished component.
        //
        // Only active-stack edges may lower a lowlink. Admitting this cross edge would
        // leave vertex 0 on the pending stack and omit it from the component result.
        let adjacency = vec![vec![Vertex(1), Vertex(2)], vec![Vertex(2)], vec![Vertex(1)]];
        assert_eq!(
            normalized(&adjacency),
            vec![vec![Vertex(0)], vec![Vertex(1), Vertex(2)]],
            "a cross edge into a finished component leaves both components intact"
        );
    }

    #[test]
    fn a_later_child_subtree_lowers_the_parent_lowlink()
    {
        // Vertex 1 first reaches terminal 2, then reaches ancestor 0 through 3.
        //
        // The later child must still propagate its lowlink into the resumed parent.
        // Otherwise component {0, 1, 3} incorrectly splits into {0} and {1, 3}.
        let adjacency = vec![
            vec![Vertex(1)],
            vec![Vertex(2), Vertex(3)],
            Vec::new(),
            vec![Vertex(0)],
        ];
        assert_eq!(
            normalized(&adjacency),
            vec![vec![Vertex(0), Vertex(1), Vertex(3)], vec![Vertex(2)]],
            "the cycle closed through a later child subtree is one component"
        );
    }
}
