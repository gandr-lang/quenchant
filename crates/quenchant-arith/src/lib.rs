//! Profile-independent arithmetic for nominal integer values.
//!
//! [`arith`] owns the complete source surface and its executable
//! specifications.
#![cfg_attr(not(feature = "anodized"), no_std)]

#[cfg(all(test, not(feature = "anodized")))]
extern crate std;

pub mod arith;
