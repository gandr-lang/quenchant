// aux-build: clause_helpers.rs
// The clause-cost gate follows calls into another crate's bodies.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_clause_traversal))]
#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    allow(specification_present, primitive_signature)
)]
#![allow(dead_code)]

extern crate clause_helpers;

use anodized::spec;
use clause_helpers::Arena;
use clause_helpers::Ledger;
use clause_helpers::Recorded;
use clause_helpers::Shelf;
use clause_helpers::tally;

/// State that holds a foreign ledger.
#[derive(Clone, Debug, Default)]
struct Book
{
    ledger: Ledger,
    pages: Vec<u32>,
}

impl Book
{
    // Refused: a foreign body walks the receiver's state.

    #[spec(ensures: |ret| ret == self.ledger.total())]
    fn total(&self) -> u32
    {
        0_u32
    }

    #[spec(ensures: |ret| ret == self.ledger.size())]
    fn size(&self) -> usize
    {
        0_usize
    }

    #[spec(ensures: |ret| ret == tally(&self.pages))]
    fn pages(&self) -> usize
    {
        self.pages.len()
    }

    #[spec(ensures: |ret| ret == self.ledger.recorded())]
    fn recorded(&self) -> usize
    {
        0_usize
    }

    // Accepted: a bounded foreign body, a stated bound, and a walk of an
    // input.

    #[spec(ensures: |ret| ret == self.ledger.newest())]
    fn newest(&self) -> Option<u32>
    {
        None
    }

    #[spec(ensures: |ret| ret == self.ledger.since(0_usize))]
    fn since(&self) -> u32
    {
        0_u32
    }

    #[spec(ensures: |ret| ret == tally(values))]
    fn count(
        &self,
        values: &[u32],
    ) -> usize
    {
        values.len()
    }
}

// Refused: a trait method that resolves to no single body.
#[spec(ensures: |ret| ret == records.recorded())]
fn recorded<R: Recorded>(records: &mut R) -> usize
{
    records.recorded()
}

// Accepted: the trait method's declaration states the bound.
#[spec(ensures: |ret| ret == records.latest())]
fn latest<R: Recorded>(records: &mut R) -> Option<u32>
{
    records.latest()
}

// Refused, naming the cause: the dependency does not export the trait whose
// method states the bound, so its metadata carries no rustdoc for it (#114).
#[spec(ensures: |ret| ret == arena.command(0_usize))]
fn command(arena: &mut Arena) -> Option<u32>
{
    arena.command(0_usize)
}

// Accepted: the same shape with the trait exported, so the bound its method
// states is in the dependency's metadata (#114).
#[spec(ensures: |ret| ret == shelf.cell(0_usize))]
fn cell(shelf: &mut Shelf) -> Option<u32>
{
    shelf.cell(0_usize)
}

fn main()
{
}
