#![allow(dead_code)]
//! A crate root in the vocabulary's shape: a lead paragraph, then each section
//! once, beside the evidence sections.
//!
//! Fenced code holds code, and a code span hides what it carries:
//!
//! ```text
//! # A `#` line inside a fence.
//! Choice: fenced, see #12.
//! ```
//!
//! The `Reversal:` label and the issue `#5` in code spans are code; the
//! label inside a sentence, as in the Reversal: here, and the plural
//! Reversals are no label, and &#35; is a character reference.
//!
//! # Examples
//!
//! - Route a frame by its kind.
//!
//! # Witnesses
//!
//! - The router's tests.
//!
//! # Boundaries
//!
//! - A frame over the wire's limit is refused before routing.
//!
//! # Items
//!
//! - [`Router`]: the router.
//!
//! # Dependencies
//!
//! - None beyond the standard library.
//!
//! # References
//!
//! - The wire format's specification.
//!
//! # Specification
//!
//! - ensures: an evidence section an item reader owns.
//!
//! # Adequacy
//!
//! # Errors
//!
//! # Panics
//!
//! # Safety

/// The router.
///
/// # Why an item heading passes
///
/// Item documentation is read for decision records, issue references and
/// history prose, never for its headings.
pub struct Router;

/// A module whose sections are fold-in spellings.
pub mod folded
{
    //! A module whose headings fold into the vocabulary.
    //!
    //! # Example
    //!
    //! # Specification attributes
    //!
    //! # Exit codes
}

/// A module whose witnesses section is another fold-in.
pub mod evidence
{
    //! # Specification and evidence
    //!
    //! # Boundary wrappers
}

/// A module whose documentation is its lead paragraph alone.
pub mod handler
{
    //! Handles one kind of frame.
}

fn main()
{
}
