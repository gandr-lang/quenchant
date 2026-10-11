// The clause-cost gate, opted into at the crate root.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_clause_traversal))]
// Specification blocks and nominal signatures are other gates' concern.
#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    allow(specification_present, primitive_signature)
)]
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anodized::spec;

/// State that grows across calls.
#[derive(Clone, Debug, Default, PartialEq)]
struct Slope
{
    cells: Vec<u32>,
    frontiers: Vec<usize>,
    keys: BTreeSet<u32>,
    table: BTreeMap<u32, u32>,
    fixed: [u32; 4],
}

impl Slope
{
    // Refused: a walk over state reached through the receiver.

    #[spec(ensures: |ret| ret == self.cells.iter().any(|&cell| cell == value))]
    fn holds(
        &self,
        value: u32,
    ) -> bool
    {
        self.keys.contains(&value)
    }

    #[spec(ensures: |ret| ret.is_none_or(|index| self.frontiers.contains(&index)))]
    fn nearest(&self) -> Option<usize>
    {
        self.frontiers.last().copied()
    }

    #[spec(requires: self.frontiers.is_sorted())]
    fn top(&self) -> Option<usize>
    {
        self.frontiers.last().copied()
    }

    #[spec(captures: before = self.cells.clone(), ensures: |_| self.cells.len() == before.len().saturating_add(1))]
    fn push(
        &mut self,
        value: u32,
    )
    {
        self.cells.push(value);
    }

    #[spec(ensures: |ret| *ret == self.frontiers)]
    fn frontiers(&self) -> Vec<usize>
    {
        self.frontiers.clone()
    }

    #[spec(ensures: |ret| { let mut count = 0_usize; for _ in &self.cells { count = count.saturating_add(1); } ret == count })]
    fn count(&self) -> usize
    {
        self.cells.len()
    }

    #[spec(ensures: |ret| ret == self.snapshot().iter().count())]
    fn recount(&self) -> usize
    {
        self.cells.len()
    }

    #[cfg_attr(all(), spec(ensures: |ret| ret == self.table.values().count()))]
    fn entries(&self) -> usize
    {
        self.table.len()
    }

    // Accepted: bounded reads of the same state.

    #[spec(ensures: |ret| ret.is_none_or(|index| self.frontiers.binary_search(&index).is_ok() && self.frontiers.last() == Some(&index)))]
    fn last(&self) -> Option<usize>
    {
        self.frontiers.last().copied()
    }

    #[spec(ensures: |ret| ret == (self.keys.contains(&key) || self.table.get(&key).is_some()))]
    fn known(
        &self,
        key: u32,
    ) -> bool
    {
        self.keys.contains(&key) || self.table.contains_key(&key)
    }

    #[spec(ensures: |ret| ret == self.fixed.iter().copied().sum::<u32>())]
    fn fixed_total(&self) -> u32
    {
        self.fixed.iter().copied().sum()
    }

    #[spec(ensures: |ret| ret.iter().all(|&cell| cell > 0) && ret.len() <= self.cells.len())]
    fn positive(&self) -> Vec<u32>
    {
        self.cells
            .iter()
            .copied()
            .filter(|&cell| cell > 0)
            .collect()
    }

    #[spec(ensures: |ret| ret == tally(&self.cells))]
    fn tallied(&self) -> usize
    {
        self.cells.len()
    }

    #[expect(
        spec_clause_traversal,
        reason = "the fixture's slope never holds more than four cells"
    )]
    #[spec(ensures: |ret| ret == self.cells.iter().count())]
    fn excepted(&self) -> usize
    {
        self.cells.len()
    }

    // Snapshot of the cells, recomputed on every call.
    fn snapshot(&self) -> Vec<u32>
    {
        self.cells.clone()
    }
}

// Refused: a walk over a mutable parameter.
#[spec(ensures: |_| cells.iter().any(|&cell| cell == value))]
fn append(
    cells: &mut Vec<u32>,
    value: u32,
)
{
    cells.push(value);
}

// Accepted: a walk over a shared input.
#[spec(ensures: |ret| ret == values.iter().copied().fold(0_u32, u32::saturating_add))]
fn total(values: &[u32]) -> u32
{
    values.iter().copied().fold(0_u32, u32::saturating_add)
}

// A helper the clause calls is not followed.
fn tally(cells: &[u32]) -> usize
{
    cells.iter().count()
}

fn main()
{
}
