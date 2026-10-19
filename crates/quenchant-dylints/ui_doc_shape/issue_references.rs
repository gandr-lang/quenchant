#![allow(dead_code)]
//! A crate root citing an issue as its design's origin (#577).
//!
//! The reader follows gandr-lang/gandr#12, and `#5` in a code span, &#35; a
//! character reference, x#6 glued to a word and a/b/c#7 in a path are none.
//!
//! ```text
//! #8
//! ```

/// The reader, shaped by #41 and #42.
pub struct Reader;

/// A module whose own documentation cites an issue.
pub mod frame
{
    //! Frames, as #9 settled.
}

fn main()
{
}
