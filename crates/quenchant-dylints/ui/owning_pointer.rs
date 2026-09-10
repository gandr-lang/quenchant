#![allow(dead_code, specification_present)]

use std::collections::HashMap;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;

#[repr(transparent)]
#[derive(Eq, Hash, PartialEq)]
struct Payload(u32);

#[repr(transparent)]
struct NodeId(u32);

// Denied: the direct `Box` self-cycle.

#[repr(transparent)]
struct BoxedSelf(Box<BoxedSelf>);

// Denied: the mutual cycle through two types.

#[repr(transparent)]
struct MutualLeft(Box<MutualRight>);

#[repr(transparent)]
struct MutualRight(Box<MutualLeft>);

// Denied: `Vec<Self>` closes the cycle without naming a pointer, which is what
// a lint quantifying over `Box`, `Rc` and `Arc` by name cannot see.

#[repr(transparent)]
struct VecSelf
{
    children: Vec<VecSelf>,
}

// Denied: `Arc` in a recursive position. `Arc` is for whole values at
// boundaries, never for the edges of recursive data.

#[repr(transparent)]
struct ArcCycle(Arc<ArcCycle>);

// Denied: the cons list, whose cycle runs through `Option` and `Rc`.

struct RcList
{
    value: Payload,
    rest: Option<Rc<RcList>>,
}

// Denied: the `Option` wrapper names no pointer, but it owns its contents;
// the `Box` beneath it closes the cycle.

#[repr(transparent)]
struct OptionBoxSelf(Option<Box<OptionBoxSelf>>);

// Denied: every variant shape of an enum, including the alternatives vector.

enum Pattern
{
    Wildcard,
    Named(Payload),
    As(Box<Pattern>),
    Alternatives(Vec<Pattern>),
}

// Denied: the map's value position owns its way back.

#[repr(transparent)]
struct Scope
{
    entries: HashMap<Payload, Scope>,
}

// Denied: a local generic whose parameter occupies an owned field position.
// The generic itself is not denied — it reaches nothing on its own.

#[repr(transparent)]
struct Owner<Row>(Box<Row>);

#[repr(transparent)]
struct OwnedThroughGeneric(Owner<OwnedThroughGeneric>);

// Denied conservatively: an external generic with no allow-list entry owns
// every one of its arguments, whatever it does with them.

#[repr(transparent)]
struct RawLinked(Option<NonNull<RawLinked>>);

// Accepted: the flat id-addressed form, which is the sanctioned shape.

#[repr(transparent)]
struct Node
{
    children: Vec<NodeId>,
}

#[repr(transparent)]
struct NodeArena
{
    nodes: Vec<Node>,
}

// Accepted: a table that merely holds members of a cycle is not itself on one.
// The cycle does not reach back to the table, so the table is its own
// component — an arena is the target state, not a row of the ledger.

#[repr(transparent)]
struct CycleTable
{
    rows: Vec<BoxedSelf>,
}

// Accepted: a non-owning reference closes no cycle.

struct Borrowed<'arena>
{
    parent: Option<&'arena Borrowed<'arena>>,
    payload: Payload,
}

// Accepted: `Weak` does not keep its pointee alive, so it drops nothing.

struct Backlink
{
    parent: Option<Weak<Backlink>>,
    payload: Payload,
}

// Accepted: `Box` of a slice of non-recursive elements. `Box` remains ordinary
// owned indirection; only the reachability of the defining type is the finding.

#[repr(transparent)]
struct Binders(Box<[NodeId]>);

// Accepted: a typed index whose parameter lives only in `PhantomData`, which is
// the shape of every typed arena id.

struct TypedId<Row>
{
    index: u32,
    row: PhantomData<Row>,
}

#[repr(transparent)]
struct TypedNode
{
    children: Vec<TypedId<TypedNode>>,
}

// Accepted: a local generic whose parameter reaches only a reference.

#[repr(transparent)]
struct Viewer<'source, Row>(&'source Row);

#[repr(transparent)]
struct Viewed<'source>(Viewer<'source, Viewed<'source>>);

// Accepted: a function pointer owns nothing it mentions.

#[repr(transparent)]
struct Continuation(fn(Continuation) -> Payload);

// Accepted, and recorded as a blind spot: a cycle closed through a trait object
// names no type, so the walk cannot follow it.

#[repr(transparent)]
struct DynCycle
{
    step: Box<dyn Fn(&DynCycle)>,
}

fn main()
{
}
