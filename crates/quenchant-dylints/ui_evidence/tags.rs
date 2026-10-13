//! A tag table and two preconditions whose executable form copies its range
//! while their prose links the table.
#![allow(
    dead_code,
    primitive_signature,
    single_field_struct_needs_transparent_repr
)]

use anodized::spec;

/// One row of the tag table.
pub struct Row
{
    /// The tag.
    pub tag: u8,
    /// Whether the tag's inline payload has a finite token bound.
    pub bounded: bool,
}

/// The table row of a tag.
///
/// # Specification
/// - requires: `tag` is a native node tag, a former [`NODE_TAG_TABLE`] lists.
/// - ensures: the row lists `tag`.
///
/// # Adequacy
/// - hypothesis: L3 — every listed tag finds its row.
/// - witness: `tests::rows`
#[spec(requires: (0x60..=0x62).contains(&tag), ensures: |output| output.tag == tag)]
pub fn row(tag: u8) -> Row
{
    Row {
        tag,
        bounded: tag != 0x62,
    }
}

/// Whether a tag's payload is unbounded.
///
/// # Specification
/// - requires: `tag` names a native former [`NODE_TAG_TABLE`] lists whose
///   inline payload has no finite token bound.
/// - ensures: returns true.
///
/// # Adequacy
/// - hypothesis: L3 — the unbounded tag is unbounded.
/// - witness: `tests::unbounded`
#[spec(requires: tag == 0x62)]
pub fn unbounded(tag: u8) -> bool
{
    !row(tag).bounded
}

/// Every native node tag.
pub const NODE_TAG_TABLE: [Row; 3] = [
    Row {
        tag: 0x60,
        bounded: true,
    },
    Row {
        tag: 0x61,
        bounded: true,
    },
    Row {
        tag: 0x62,
        bounded: false,
    },
];

/// The fixture compiles as a program.
///
/// # Specification
/// trivial.
fn main()
{
}
