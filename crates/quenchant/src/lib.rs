//! The specification attribute under the `quenchant` namespace.
//!
//! Every item is a re-export. [`spec`] is the specification attribute: its
//! expansion writes `::quenchant::` helper paths, so the helpers are
//! re-exported beside it and resolve through this package.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![no_std]
#![forbid(unsafe_code)]

#[doc(hidden)]
pub use quenchant_anodized::__erase;
#[cfg(feature = "anodized")]
#[doc(hidden)]
pub use quenchant_anodized::__instrument;
pub use quenchant_anodized::spec;
