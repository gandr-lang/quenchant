//! One integration target gives every gate witness a stable suite-qualified
//! path.
//!
//! File modules are declared once here under `#[cfg(test)]`. Their witness
//! paths are `gates::<file>::<test>`, and their code remains subject to the
//! library's lint policy.

#[cfg(test)]
mod anodized;
#[cfg(test)]
mod catalog;
#[cfg(test)]
mod witnesses;
