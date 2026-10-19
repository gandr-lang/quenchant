// compile-flags: --test
#![allow(dead_code)]
#![warn(module_doc_measure)]
//! A crate root measured.
//!
//! # Decisions
//!
//! - Choice: one row.
//! - Alternatives: another.
//! - Reversal: a change.

/// A module measured: one line, no decision.
pub mod measured
{
    //! One line.
}

/// A module without documentation of its own is not measured.
pub mod undocumented
{}
