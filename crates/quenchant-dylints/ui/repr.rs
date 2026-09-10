#![allow(dead_code, specification_present)]

struct MissingNamed
{
    value: u8,
}

struct MissingTuple(u16);

#[repr(transparent)]
struct TransparentNamed
{
    value: u32,
}

#[repr(transparent)]
struct TransparentTuple(u64);

struct TwoFields
{
    left: u8,
    right: u8,
}

struct Unit;

macro_rules! generated_single_field_struct {
    () => {
        struct MacroGenerated(u8);
    };
}

generated_single_field_struct!();

fn main()
{
}
