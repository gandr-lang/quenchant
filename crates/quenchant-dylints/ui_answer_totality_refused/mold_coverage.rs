#![allow(dead_code, specification_present, primitive_signature)]

/// The nodes a printer writes.
enum Node
{
    Unit,
    Pair(u8, u8),
    Lazy,
}

/// The marker: a production names the rule it spells.
#[repr(transparent)]
struct Spells(&'static str);

/// The unit names its rule; the lazy node names a rule no builder defines;
/// the pair names its rule through a variable, which cannot be read; the
/// record and the pair are left without a production.
fn print(node: &Node) -> Spells
{
    let pair = "pair_type";
    match *node {
        Node::Unit => Spells("unit_type"),
        Node::Pair(..) => Spells(pair),
        Node::Lazy => Spells("lazy_type"),
    }
}

fn main()
{
}
