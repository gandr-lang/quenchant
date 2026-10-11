#![no_std]
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(
    not(doc),
    doc = "Specification attributes with optional development instrumentation."
)]

#[cfg(all(any(anodized_panic, anodized_print), not(feature = "anodized")))]
compile_error!(
    "ANODIZED_BACKEND_DISABLED: anodized_panic or anodized_print requires the quenchant-anodized/anodized feature"
);

#[cfg(feature = "anodized")]
#[doc(hidden)]
pub use anodized::__;
#[cfg(feature = "anodized")]
pub use anodized::result;
#[cfg(feature = "anodized")]
pub use anodized::types;
#[cfg(feature = "anodized")]
pub use anodized_macros::spec;
#[cfg(not(feature = "anodized"))]
#[doc(hidden)]
pub use quenchant_spec_macros::__erase;
#[cfg(not(feature = "anodized"))]
pub use quenchant_spec_macros::spec;
