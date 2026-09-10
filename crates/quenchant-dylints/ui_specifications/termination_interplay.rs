// Specification expansion preserves complete and incomplete termination sections.
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

/// Consume a private closed fuel supply.
///
/// # Termination
/// - reason: the recursion consumes the private closed `Fuel` enum, not input.
/// - measure: remaining `Fuel` variants before `Zero`.
/// - boundedness: `One` strictly recurses to `Zero`, and `Zero` returns.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "approved closed-fuel recursion")]
#[quenchant::spec(ensures: |output| output >= Count::ZERO)]
fn complete(fuel: Fuel) -> Count {
    match fuel {
        Fuel::One => complete(Fuel::Zero),
        Fuel::Zero => Count::ZERO,
    }
}

/// An incomplete section remains incomplete through expansion.
///
/// # Termination
/// - reason: the recursion consumes the private closed `Fuel` enum, not input.
/// - measure: remaining `Fuel` variants before `Zero`.
#[expect(recursion_forbidden, reason = "approved closed-fuel recursion")]
#[quenchant::spec(ensures: |output| output >= Count::ZERO)]
fn truncated(fuel: Fuel) -> Count {
    match fuel {
        Fuel::One => truncated(Fuel::Zero),
        Fuel::Zero => Count::ZERO,
    }
}

fn main() {}
