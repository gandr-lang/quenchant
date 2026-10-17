#![allow(dead_code, specification_present, primitive_signature)]

/// The nodes a presenter writes.
enum Node
{
    Unit,
    Pair(u8, u8),
    Record
    {
        width: u8,
    },
    Lift,
}

/// The text one presenter writes.
#[repr(transparent)]
struct Text(&'static str);

/// The presenter.
struct Walker;

impl Walker
{
    /// The record reaches the wildcard and the lift the bare binding, each
    /// in a function of its own; neither is named.
    fn visit(
        &self,
        node: &Node,
    ) -> Text
    {
        match *node {
            Node::Unit => Text("unit"),
            Node::Pair(..) => Text("(_, _)"),
            _ => Text("?"),
        }
    }

    /// A bare binding names nothing.
    fn form(
        &self,
        node: &Node,
    ) -> Text
    {
        match node {
            Node::Unit => Text("unit"),
            other => self.visit(other),
        }
    }
}

fn main()
{
}
