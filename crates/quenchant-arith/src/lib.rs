//! Nominal integer arithmetic with an explicit overflow policy.
//!
//! [`arith`] contains the representation boundaries, named families, and
//! executable specification predicates.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(not(anodized_print), no_std)]

#[cfg(all(test, anodized_panic))]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arith;
