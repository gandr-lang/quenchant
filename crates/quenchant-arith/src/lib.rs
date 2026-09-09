//! Nominal integer arithmetic with an explicit overflow policy.
//!
//! [`arith`] contains the representation boundaries, named families, and
//! executable specification predicates.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "anodized"), no_std)]

#[cfg(all(test, not(feature = "anodized")))]
extern crate std;

pub mod arith;
