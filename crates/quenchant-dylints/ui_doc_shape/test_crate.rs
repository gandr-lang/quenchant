// compile-flags: --test
#![allow(dead_code)]
#![warn(doc_history_prose, module_doc_measure)]
//! A test compilation is not read: a regression test names the defect it pins
//! (#577), and its documentation may narrate what used to fail.
//!
//! # Status
//!
//! - Choice: a status section.
//! - Alternatives: none.
//! - Reversal: never.

/// The regression for #578, which no longer refuses.
#[test]
fn regression()
{
}
