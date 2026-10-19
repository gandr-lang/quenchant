// compile-flags: --test
#![allow(dead_code)]
//! A crate root in the sanctioned shape: a lead paragraph, then the module
//! headings, each once, beside an evidence heading.
//!
//! Fenced code holds code:
//!
//! ```text
//! # A `#` line inside a fence.
//! previously = formerly
//! ```
//!
//! # Decisions
//!
//! - Choice: one, at the cap of seven.
//! - Alternatives: another.
//! - Reversal: a change.
//! - Choice: two, its alternative singular.
//! - Alternative: another.
//! - Reversal: a change.
//! - Choice: three, its reversal wrapped across two lines.
//! - Alternatives: another.
//! - Reversal: a change that continues.
//! - Choice: four.
//! - Alternatives: another.
//! - Reversal: a change.
//!
//! - Choice: five, after a blank line.
//! - Alternatives: another.
//! - Reversal: a change.
//! - Choice: six.
//! - Alternatives: another.
//! - Reversal: a change.
//! - Choice: seven.
//! - Alternatives: another.
//! - Reversal: a change.
//!
//! # Items
//!
//! - [`Router`]: the router; it is used to dispatch every kind.
//!
//! # References
//!
//! - The wire format's specification.
//!
//! # Specification
//!
//! - ensures: an evidence section another reader owns.

/// The router.
///
/// # Why an item heading passes
///
/// Item documentation is read for history prose alone.
pub struct Router;

/// A module whose documentation is its lead paragraph alone.
pub mod handler
{
    //! Handles one kind of frame.
}
