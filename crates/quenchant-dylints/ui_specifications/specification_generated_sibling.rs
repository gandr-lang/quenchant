// The presence rule reads the author's own declaration, and not the sibling a
// foreign expansion generates under that declaration's name. A crate-local
// expansion is not foreign and stays inside the rule.
#![allow(dead_code)]

use quenchant_fixture_macros::client;

/// The subject whose declarations the client expansion mirrors.
struct Turn;

#[client]
impl Turn {
    /// Persist the turn's accepted transition.
    ///
    /// # Specification
    /// - ensures: nothing observable.
    /// - panics: none.
    pub fn change(&self) {}
}

/// Expand a function whose block this macro's author never wrote.
macro_rules! undocumented_expansion {
    ($name:ident) => {
        fn $name() {}
    };
}

undocumented_expansion!(from_a_local_macro);

/// The fixture's entry point.
///
/// # Specification
/// trivial.
fn main() {}
