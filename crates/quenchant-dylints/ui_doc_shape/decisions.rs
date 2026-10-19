// compile-flags: --test
#![allow(dead_code)]
//! A crate root whose decisions are not rows.
//!
//! **Choice.** A decision labelled outside `# Decisions` is refused.
//! **Reversal.** Its reversal closes it, so it is one decision.
//!
//! # Decisions
//!
//! - Choice: a row with a fourth bullet.
//! - Alternatives: three bullets, which the row carries.
//! - Reversal: never.
//! - Alternatives: the fourth bullet, which no row carries.
//! - Choice: a row out of order.
//! - Reversal: written before the alternatives.
//! - Alternatives: written last.
//! - Choice: a row with an empty bullet.
//! - Alternatives:
//! - Reversal: an alternative stated.
//! - **A bold lead.** The bullet opens with no label.
//!
//! A paragraph under `# Decisions` is not a row,
//! however many lines it runs.
//!
//! ## Eviction
//!
//! # Items
//!
//! - [`Cache`]: the cache.

/// The cache the decisions describe.
pub struct Cache;

/// A module recording one decision more than the cap.
pub mod crowded
{
    //! Eight rows, each well formed.
    //!
    //! # Decisions
    //!
    //! - Choice: one.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: two.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: three.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: four.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: five.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: six.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: seven.
    //! - Alternatives: another.
    //! - Reversal: a change.
    //! - Choice: eight.
    //! - Alternatives: another.
    //! - Reversal: a change.
}
