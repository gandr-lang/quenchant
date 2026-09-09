//! Specification-first hardening for Rust: types, obligations, tests, and gates
//! that make evidence explicit.
//!
//! Every item is a re-export. [`arith`] and [`shape`] are the member libraries'
//! own modules, and [`spec`] is the specification attribute: its expansion
//! writes `::quenchant::` helper paths, so the helpers are re-exported beside
//! it and resolve through this package.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![no_std]
#![forbid(unsafe_code)]

#[doc(hidden)]
pub use quenchant_anodized::__erase;
#[cfg(feature = "anodized")]
#[doc(hidden)]
pub use quenchant_anodized::__instrument;
pub use quenchant_anodized::spec;
pub use quenchant_arith::arith;
#[cfg(feature = "gates")]
pub use quenchant_gates as gates;
pub use quenchant_shape::delegate_ops;
pub use quenchant_shape::nominal_type;
pub use quenchant_shape::reason_enum;
pub use quenchant_shape::shape;
