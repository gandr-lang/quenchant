//! One integration target gives every gate witness a stable suite-qualified
//! path.
//!
//! File modules are declared once here under `#[cfg(test)]`. Their witness
//! paths are `gates::<file>::<test>`, and their code remains subject to the
//! library's lint policy.

#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    deny(
        spec_attribute_present,
        adequacy_present,
        maybe_shape,
        erased_error_signature,
        spec_attribute_unqualified
    )
)]

#[cfg(test)]
mod anodized;
#[cfg(test)]
mod catalog;
#[cfg(test)]
mod facade;
#[cfg(test)]
mod repository;
#[cfg(test)]
mod witnesses;
