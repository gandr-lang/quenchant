//! Nominal non-failure absence and explicit primitive-border scaffolding.
#![cfg_attr(not(feature = "anodized"), no_std)]
#![forbid(unsafe_code)]

#[cfg(all(test, not(feature = "anodized")))]
extern crate std;

pub mod shape;
