#![allow(dead_code, specification_present, primitive_signature)]

/// The nodes a printer writes.
enum Node
{
    Unit,
    Pair(u8, u8),
    Record
    {
        width: u8,
    },
}

/// The text one production writes, with the rule it spells.
struct Spelled
{
    rule: &'static str,
    text: &'static str,
}

/// The marker: a production names the rule it spells.
fn spells(
    rule: &'static str,
    text: &'static str,
) -> Spelled
{
    Spelled { rule, text }
}

/// Every rule but the shebang, which no printer writes, has its production.
fn print(node: &Node) -> Spelled
{
    match *node {
        Node::Unit => spells("unit_type", "Unit"),
        Node::Pair(..) => spells("pair_type", "(_, _)"),
        Node::Record { .. } => spells("record_type", "#{..}"),
    }
}

fn main()
{
}
