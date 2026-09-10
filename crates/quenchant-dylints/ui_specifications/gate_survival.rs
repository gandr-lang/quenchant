// Boundary gates must survive the real specification expansion.
#![allow(dead_code, specification_present)]
#![allow(unconditional_recursion)]

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Count(u32);

impl Count {
    const ZERO: Self = Self(0);
}

enum Fuel {
    One,
    Zero,
}

/// The recursive call inside the generated closure still belongs to this function.
#[quenchant::spec(ensures: |output| output >= Count::ZERO)]
fn specified_recursion(fuel: Fuel) -> Count {
    match fuel {
        Fuel::One => specified_recursion(Fuel::Zero),
        Fuel::Zero => Count::ZERO,
    }
}

/// The ordinary recursion control is denied too.
fn ordinary_recursion(fuel: Fuel) -> Count {
    match fuel {
        Fuel::One => ordinary_recursion(Fuel::Zero),
        Fuel::Zero => Count::ZERO,
    }
}

/// Author type tokens retain the primitive-signature finding.
#[quenchant::spec(requires: value > 0)]
fn specified_primitive(value: u32) -> u32 {
    value
}

/// The ordinary signature control is denied too.
fn ordinary_primitive(value: u32) -> u32 {
    value
}

fn main() {}
