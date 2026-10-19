#![allow(dead_code)]
//! A crate root recording its decisions.
//!
//! - Choice: one cache per process.
//! - Alternatives: a shared cache, whose invalidation costs more.
//! - Reversal: a rebuild slower than the invalidation.
//!
//! **The reader owns its buffer.** A borrowed buffer ties the frame to the
//! socket. Alternative: a pool. Reversal: a pool that never contends.
//!
//! |Choice|Alternative|Reversal condition|
//! |---|---|---|
//! |one cache|a shared cache|a slow rebuild|

/// The cache, with a decision in its item documentation.
///
/// **Choice.** One per process (Reversal: a shared store).
pub struct Cache;

/// A function whose documentation opens a decision section.
///
/// # Decisions
///
/// - Alternatives: an inline copy.
fn load()
{
}

/// A module whose decision has no choice.
pub mod reader
{
    //! Reads frames.
    //!
    //! Reversal: a reader that never blocks.
}

fn main()
{
}
