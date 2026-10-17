#![allow(dead_code, specification_present, primitive_signature)]

/// Why a node reads as nothing the printer spells.
#[derive(Clone, Copy)]
enum Unreadable
{
    Dangling,
    Hole,
}

/// What the printer reads of a node.
#[derive(Clone, Copy)]
enum Former
{
    Unit,
    Unreadable(Unreadable),
    Unknown
    {
        width: u8,
    },
}

/// The placeholder node, as a unit struct.
struct Placeholder;

/// The text one leaf writes.
#[repr(transparent)]
struct Glyphs(&'static str);

/// A unit variant of a configured enum.
fn unit_variant() -> Unreadable
{
    Unreadable::Dangling
}

/// A configured tuple variant called, over a unit variant of a configured
/// enum.
fn called() -> Former
{
    Former::Unreadable(Unreadable::Hole)
}

/// A configured tuple variant passed as a function.
fn passed(reasons: Vec<Unreadable>) -> Vec<Former>
{
    reasons.into_iter().map(Former::Unreadable).collect()
}

/// A configured struct variant.
fn struct_variant() -> Former
{
    Former::Unknown { width: 1 }
}

/// A configured unit struct.
fn unit_struct() -> Placeholder
{
    Placeholder
}

/// The configured spelling.
fn spelled() -> Glyphs
{
    Glyphs("?")
}

/// An unconfigured sibling is not read.
fn sibling() -> Former
{
    Former::Unit
}

/// An expectation without its reason.
#[expect(dishonest_answer)]
fn unreasoned() -> Unreadable
{
    Unreadable::Hole
}

/// An expectation inherited from the enclosing module.
mod inherited
{
    #![expect(
        answer_totality,
        reason = "the module states one reason for all it holds"
    )]

    /// A placeholder under the module's expectation.
    fn inside() -> super::Unreadable
    {
        super::Unreadable::Hole
    }
}

/// An `allow` admits without stating why.
#[allow(answer_totality)]
fn allowed() -> Unreadable
{
    Unreadable::Hole
}

fn main()
{
}
