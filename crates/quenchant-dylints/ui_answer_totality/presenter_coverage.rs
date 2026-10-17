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
    Single(u8),
    #[expect(
        presenter_coverage,
        reason = "a lift has no surface spelling; the runtime sweep counts it"
    )]
    Lift,
}

/// The text one presenter writes.
#[repr(transparent)]
struct Text(&'static str);

/// The presenter.
struct Walker;

impl Walker
{
    /// Every variant but the lift is named here or in `form`; the wildcard
    /// reaches only names `form` writes and the lift.
    fn visit(
        &self,
        node: &Node,
    ) -> Text
    {
        match *node {
            Node::Unit => Text("unit"),
            Node::Pair(..) | Node::Single(_) => self.form(node),
            _ => Text("#{}"),
        }
    }

    /// A variant named in a nested pattern, an `if let` and a closure counts.
    fn form(
        &self,
        node: &Node,
    ) -> Text
    {
        if let Node::Record { .. } = *node {
            return Text("#{..}");
        }
        let pair = |node: &Node| matches!((node, 0_u8), (Node::Pair(..), _));
        if pair(node) {
            Text("(_, _)")
        }
        else {
            Text("(..)")
        }
    }
}

fn main()
{
}
