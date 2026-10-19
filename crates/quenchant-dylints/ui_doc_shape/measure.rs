#![allow(dead_code, doc_decision_record)]
#![warn(module_doc_measure)]
//! A crate root measured.
//!
//! # Items
//!
//! - Choice: one record, counted.
//! - Alternatives: another.
//! - Reversal: a change.

/// A module measured: one line, no section, no record.
pub mod measured
{
    //! One line.
}

/// A module without documentation of its own is not measured.
pub mod undocumented
{}

fn main()
{
}
