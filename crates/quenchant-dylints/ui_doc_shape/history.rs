// compile-flags: --test
#![allow(dead_code)]
//! A crate root reading frame headers; the checksum is no longer verified.
//!
//! A header is used to route its frame, and the reader used to buffer it.

/// The reader. Previously it owned its socket.
pub struct Reader;

/// The header, as of the second wire version.
pub struct Header;

/// A field that was renamed in the second version; `formerly` in a code span
/// is an identifier.
pub struct Field;

/// A buffer that the reader no
/// longer clears: a phrase wrapped across lines is found at its first word,
/// and a buffer no longer than one frame is a comparison.
pub struct Buffer;

/// A fence holds code:
///
/// ```text
/// previously = formerly
/// ```
pub struct Fenced;

/// A module whose own documentation narrates.
pub mod legacy
{
    //! Historically part of the reader.
}

/// Wraps a struct in the documentation its invocation carries.
macro_rules! documented {
    ($(#[$meta:meta])* struct $name:ident;) => {
        $(#[$meta])*
        pub struct $name;
    };
}

documented!(
    /// A comment a macro carries is reported at its invocation, formerly at
    /// the macro's definition.
    struct Carried;
);
