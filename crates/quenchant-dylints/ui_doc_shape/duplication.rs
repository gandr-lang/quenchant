// rustc-env:CARGO_MANIFEST_DIR=ui_doc_shape/readme
#![allow(dead_code)]
//! The frame reader keeps one buffer per connection, drains each buffer in
//! arrival order and hands every complete frame to the [`Router`], which
//! chooses its handler from the frame's kind and never blocks the reader.
//!
//! A short paragraph of twenty-four words repeats in a module, and stays one
//! word short of the threshold, so no report ever names it.

/// The router the paragraph names.
pub struct Router;

/// The first module carrying a shared paragraph.
pub mod first
{
    //! Every handler receives its frame by value and owns it from then on, so
    //! a handler that stores a frame never borrows from the reader's buffer or
    //! delays its next read.
}

/// The second module carrying the same paragraph, wrapped otherwise.
pub mod second
{
    //! Every handler receives its frame by value and owns it from then on,
    //! so a handler that stores a frame never borrows from the reader's
    //! buffer or delays its next read.
    //!
    //! A short paragraph of twenty-four words repeats in a module, and stays
    //! one word short of the threshold, so no report ever names it.
}

fn main()
{
}
