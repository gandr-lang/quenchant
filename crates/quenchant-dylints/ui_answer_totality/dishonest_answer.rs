// compile-flags: --test
#![allow(dead_code, specification_present, primitive_signature)]

/// Why a node reads as nothing the printer spells.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Unreadable
{
    Dangling,
    Hole,
}

/// What the printer reads of a node.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Former
{
    Unit,
    Pair(u8, u8),
    Unreadable(Unreadable),
}

/// The text one leaf writes.
#[repr(transparent)]
#[derive(Debug, PartialEq)]
struct Glyphs(&'static str);

/// A placeholder built under a reasoned expectation on its function.
#[expect(answer_totality, reason = "a dangling handle names no node to spell")]
fn dangling() -> Former
{
    Former::Unreadable(Unreadable::Dangling)
}

/// A placeholder built under a reasoned expectation on its statement.
fn hole() -> Unreadable
{
    #[expect(answer_totality, reason = "a hole is source not yet written")]
    let hole = Unreadable::Hole;
    hole
}

/// A placeholder spelled under a reasoned expectation on its arm, naming the
/// rule rather than the group.
fn spelled(former: Former) -> Glyphs
{
    match former {
        Former::Unit => Glyphs("unit"),
        Former::Pair(..) => Glyphs("(_, _)"),
        #[expect(dishonest_answer, reason = "the reason travels beside the placeholder")]
        Former::Unreadable(_) => Glyphs("?"),
    }
}

/// Patterns and comparisons read placeholders without building them.
fn unreadable(former: Former) -> bool
{
    matches!(former, Former::Unreadable(Unreadable::Hole))
        || former == Former::Unreadable(Unreadable::Dangling)
        || spelled(former).0 == "?"
}

/// A sibling of a configured variant is an honest answer.
fn unit() -> Former
{
    Former::Unit
}

#[test]
fn an_unreadable_former_spells_the_placeholder()
{
    assert_eq!(spelled(Former::Unreadable(Unreadable::Hole)), Glyphs("?"));
}
