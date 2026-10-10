//! Specification-first hardening for Rust: types, obligations, tests, and gates
//! that make evidence explicit.
//!
//! Every item is a re-export. [`arith`] and [`shape`] are the member libraries'
//! own modules. Specification attributes come from the separate `anodized`
//! library, provided by the `quenchant-anodized` package.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![no_std]
#![forbid(unsafe_code)]

pub use quenchant_arith::arith;
#[cfg(feature = "gates")]
pub use quenchant_gates as gates;
pub use quenchant_shape::delegate_ops;
pub use quenchant_shape::nominal_type;
pub use quenchant_shape::reason_enum;
pub use quenchant_shape::shape;
