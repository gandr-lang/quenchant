//! Domain identity, reasoned absence, and explicit primitive conversion
//! boundaries.
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
#![forbid(unsafe_code)]
#![cfg_attr(quenchant_compiler_policy, feature(rustc_attrs))]
#![cfg_attr(
    quenchant_compiler_policy,
    expect(
        internal_features,
        unstable_features,
        reason = "The compiler-policy build registers canonical type identity; ordinary library builds use no compiler attributes."
    )
)]

#[cfg(all(test, anodized_panic))]
extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod shape;
