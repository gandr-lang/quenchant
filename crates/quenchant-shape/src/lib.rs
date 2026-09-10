//! Domain identity, reasoned absence, and explicit primitive conversion
//! boundaries.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "anodized"), no_std)]
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

#[cfg(all(test, not(feature = "anodized")))]
extern crate std;

pub mod shape;
