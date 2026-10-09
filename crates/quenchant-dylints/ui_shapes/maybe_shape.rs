// The hand-rolled `Maybe` gate, opted into at the crate root.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(maybe_shape))]
#![feature(rustc_attrs)]
#![allow(internal_features)]

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Count(u32);

/// A per-site reason.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MissingReason
{
    NotStored,
}

/// `Maybe` reimplemented under its own variant names.
enum Possibly<Value, Reason>
{
    Here(Value),
    Gone(Reason),
}

/// The same shape with named fields.
enum Lookup<Value, Reason>
{
    Found
    {
        value: Value
    },
    Missing
    {
        why: Reason
    },
}

/// A further parameter does not hide the shape.
enum Widened<Value, Reason, const WIDTH: usize>
{
    Present(Value),
    Absent(Reason),
}

/// A copy of the canonical definition, without its identity.
enum Maybe<Value, Reason>
{
    Present(Value),
    Absent(Reason),
}

/// A unit variant beside a payload names its state.
enum Direction<Expected>
{
    Synthesise,
    Check(Expected),
}

/// A per-site enum that fixes its reason is a closed state enum.
enum Located<Value>
{
    Found(Value),
    Missing(MissingReason),
}

/// One parameter on both sides is a pair of states over one type.
enum Side<Value>
{
    Left(Value),
    Right(Value),
}

/// A wrapped parameter is not a bare payload.
enum Boxed<Value, Reason>
{
    Held(Box<Value>),
    Dropped(Reason),
}

/// A third variant is a larger state set.
enum Three<Value, Reason>
{
    Present(Value),
    Absent(Reason),
    Pending,
}

/// A concrete payload is a closed state enum.
enum Slot
{
    Filled(Count),
    Empty(MissingReason),
}

/// The definition that registers the canonical identity.
mod canonical
{
    #[rustc_diagnostic_item = "quenchant_maybe"]
    pub enum Maybe<Value, Reason>
    {
        Present(Value),
        Absent(Reason),
    }
}

fn main()
{
}
