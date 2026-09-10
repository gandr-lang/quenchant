#![no_std]
#![forbid(unsafe_code)]

/// Unrelated types with the same crate/module/item spelling.
pub mod shape {
    /// A foreign container that does not store an absence reason.
    pub struct Maybe<Content, Reason> {
        /// The exposed payload.
        pub content: Content,
        /// A type marker, not a stored reason.
        pub marker: core::marker::PhantomData<Reason>,
    }
}
