#![allow(dead_code)]
//! A crate root opening headings outside the vocabulary.
//!
//! # Status
//!
//! A status section is refused by name.
//!
//! # Overview
//!
//! The lead is the overview.
//!
//! # Decisions
//!
//! # Items
//!
//! ## Detail
//!
//! A sub-section is refused: module documentation has one level.
//!
//! ```text
//! # A `#` line inside a fence opens no heading.
//! ```
//!
//! # The rule
//!
//! A topic heading is outside the vocabulary.
//!
//! # Example
//!
//! # Examples
//!
//! A section opened through its fold-in and again under its own spelling is
//! refused the second time.

/// A struct's own headings are not module headings.
///
/// # Notes
pub struct Frame;

/// An outer comment on a module is not the module's own documentation.
///
/// # Background
pub mod reader
{
    //! A module's own documentation is held to the same vocabulary.
    //!
    //! # History
    //!
    //! # Changelog
    //!
    //! # Notes
    //!
    //! # Background
    //!
    //! # Decision
}

fn main()
{
}
