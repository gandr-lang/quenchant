//! Domain identity, reasoned absence, and explicit primitive conversion
//! boundaries.
#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(not(feature = "anodized"), no_std)]
#![forbid(unsafe_code)]

#[cfg(all(test, not(feature = "anodized")))]
extern crate std;

pub mod shape;
