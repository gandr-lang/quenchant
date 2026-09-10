#![no_std]
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(
    not(doc),
    doc = "Specification attributes with optional published instrumentation."
)]

#[cfg(feature = "anodized")]
#[doc(hidden)]
pub use anodized_macros::spec as __instrument;
#[doc(hidden)]
pub use quenchant_spec_macros::__erase;
pub use quenchant_spec_macros::spec;
