#![no_std]
#![forbid(unsafe_code)]

/// Nominal types for the crate-identity counterexample.
pub mod shape {
    /// A reason-bearing enum with the canonical-looking path.
    pub enum Maybe<Content, Reason> {
        /// A present payload.
        Present(Content),
        /// A stored absence reason.
        Absent(Reason),
    }
}
