//! The gate crate's behavioral suite, consolidated into one target so a
//! witness path names the suite it belongs to.
//!
//! Each file's contents sit inside a `#[cfg(test)]` module named for the file,
//! declared here rather than nested a second time inside the file itself: the
//! module path a witness spells stays `gates::<file>::<test>`, and the suite
//! reads under the same wall as the library.

#[cfg(test)]
mod anodized;
#[cfg(test)]
mod catalog;
#[cfg(test)]
mod witnesses;
