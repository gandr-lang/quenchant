// compile-flags: --test
#![allow(dead_code)]
//! A crate root opening headings outside the set.
//!
//! # Status
//!
//! A status section narrates; it is refused.
//!
//! # Items
//!
//! ## Detail
//!
//! A sub-heading outside `# Decisions` is outside the set too.
//!
//! ```text
//! # A `#` line inside a fence opens no heading.
//! ```
//!
//! # Items
//!
//! A module heading opened twice is refused the second time.
//!
//! # Safety
//!
//! - unsafe invariants: an evidence heading is sanctioned in a module too.

/// A struct's own headings are not module headings.
///
/// # Why this item exists
///
/// Item documentation is read for history prose alone.
pub struct Frame;

/// An outer comment on a module is not the module's own documentation.
///
/// # Notes
pub mod reader
{
    //! A module's own documentation is held to the same set.
    //!
    //! # History
    //!
    //! A history section is refused.
}
