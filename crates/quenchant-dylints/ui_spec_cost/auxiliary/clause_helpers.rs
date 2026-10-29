// This dependency defines the helpers; only its consumer opts into the gate.
#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    allow(specification_present, primitive_signature)
)]

/// Entries that grow across calls.
#[derive(Clone, Debug, Default)]
#[repr(transparent)]
pub struct Ledger
{
    pub entries: Vec<u32>,
}

/// Counts every entry it is given.
pub fn tally(entries: &[u32]) -> usize
{
    entries.iter().count()
}

impl Ledger
{
    /// Sums every entry.
    pub fn total(&self) -> u32
    {
        self.entries.iter().copied().fold(0_u32, u32::wrapping_add)
    }

    /// Counts every entry through another function.
    pub fn size(&self) -> usize
    {
        tally(&self.entries)
    }

    /// The newest entry.
    pub fn newest(&self) -> Option<u32>
    {
        self.entries.last().copied()
    }

    /// The entries recorded after `mark`, summed.
    ///
    /// # Clause cost
    /// - bound: reads only the entries recorded after `mark`.
    pub fn since(
        &self,
        mark: usize,
    ) -> u32
    {
        self.entries
            .iter()
            .skip(mark)
            .copied()
            .fold(0_u32, u32::wrapping_add)
    }
}

/// A record source whose implementations are chosen by the caller.
pub trait Recorded
{
    /// How many records there are.
    fn recorded(&self) -> usize;

    /// The newest record.
    ///
    /// # Clause cost
    /// - bound: every implementation reads one record.
    fn latest(&self) -> Option<u32>;
}

impl Recorded for Ledger
{
    fn recorded(&self) -> usize
    {
        self.entries.iter().count()
    }

    fn latest(&self) -> Option<u32>
    {
        self.entries.last().copied()
    }
}

/// Nodes held in a family, behind a module the crate does not export.
mod nodes
{
    /// A family of nodes whose implementations are chosen by the caller.
    pub trait Nodes
    {
        /// The node at `offset`.
        ///
        /// # Clause cost
        /// - bound: every implementation reads one node.
        fn node_at(
            &self,
            offset: usize,
        ) -> Option<u32>;
    }

    impl Nodes for Vec<u32>
    {
        fn node_at(
            &self,
            offset: usize,
        ) -> Option<u32>
        {
            self.get(offset).copied()
        }
    }

    /// The node at `offset` of any family.
    pub fn read_in<Held: Nodes + ?Sized>(
        family: &Held,
        offset: usize,
    ) -> Option<u32>
    {
        family.node_at(offset)
    }
}

/// Commands held in a family.
#[derive(Clone, Debug, Default)]
#[repr(transparent)]
pub struct Arena
{
    commands: Vec<u32>,
}

impl Arena
{
    /// The command at `offset`.
    pub fn command(
        &self,
        offset: usize,
    ) -> Option<u32>
    {
        nodes::read_in(&self.commands, offset)
    }
}

/// Cells held in a shelf, behind a module the crate exports.
pub mod cells
{
    /// A shelf of cells whose implementations are chosen by the caller.
    pub trait Cells
    {
        /// The cell at `offset`.
        ///
        /// # Clause cost
        /// - bound: every implementation reads one cell.
        fn cell_at(
            &self,
            offset: usize,
        ) -> Option<u32>;
    }

    impl Cells for Vec<u32>
    {
        fn cell_at(
            &self,
            offset: usize,
        ) -> Option<u32>
        {
            self.get(offset).copied()
        }
    }

    /// The cell at `offset` of any shelf.
    pub fn read_at<Held: Cells + ?Sized>(
        shelf: &Held,
        offset: usize,
    ) -> Option<u32>
    {
        shelf.cell_at(offset)
    }
}

/// Cells held on a shelf.
#[derive(Clone, Debug, Default)]
#[repr(transparent)]
pub struct Shelf
{
    cells: Vec<u32>,
}

impl Shelf
{
    /// The cell at `offset`.
    pub fn cell(
        &self,
        offset: usize,
    ) -> Option<u32>
    {
        cells::read_at(&self.cells, offset)
    }
}
