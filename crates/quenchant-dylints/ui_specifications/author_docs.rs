//! Native documentation checks retain their authority through specification expansion.
#![allow(dead_code, specification_present)]
#![deny(missing_docs)]

/// A semantic count.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
pub struct Count(u32);

impl Count {
    /// The count of nothing.
    pub const ZERO: Self = Self(0);
}

// Denied by rustc: the macro injects no documentation to hide this omission.
#[anodized::spec(requires: count > Count::ZERO)]
pub fn undocumented(count: Count) -> Count {
    count
}

/// Author comments survive token-stream round trips.
#[anodized::spec(ensures: |output| output > Count::ZERO)]
pub fn documented(count: Count) -> Count {
    count
}

/** Block comments remain author documentation. */
#[anodized::spec]
pub fn block_comment(count: Count) -> Count {
    count
}

#[doc = "Explicit author documentation is visible to the native lint."]
#[anodized::spec]
pub fn raw_attribute(count: Count) -> Count {
    count
}

fn main() {}
