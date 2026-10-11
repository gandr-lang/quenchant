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
