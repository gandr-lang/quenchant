//! Nominal integer arithmetic with an explicit overflow policy.
//!
//! [`arith`] contains the representation boundaries, named families, and
//! executable specification predicates.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    deny(
        spec_attribute_present,
        adequacy_present,
        maybe_shape,
        erased_error_signature,
        spec_attribute_unqualified
    )
)]
#![cfg_attr(not(anodized_print), no_std)]

#[cfg(all(test, anodized_panic))]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod arith;
