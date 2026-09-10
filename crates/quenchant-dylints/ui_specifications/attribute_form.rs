// Real specification attributes preserve imports and ordinary policy checks.
#![allow(dead_code, specification_present)]
#![deny(unused_imports)]

use quenchant::spec;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Count(u32);

impl Count {
    const ZERO: Self = Self(0);
}

/// Combined clauses use one attribute, without competing predicate macros.
#[spec(requires: count > Count::ZERO, ensures: |output| output > Count::ZERO)]
fn combined(count: Count) -> Count {
    count
}

/// Qualified attributes expand identically.
#[quenchant::spec(maintains: count > Count::ZERO)]
fn qualified(count: Count) -> Count {
    count
}

/// Conditional attributes keep the same specification entry point.
#[cfg_attr(debug_assertions, spec(requires: count > Count::ZERO))]
fn conditional(count: Count) -> Count {
    count
}

/// Empty specifications still preserve the author's function.
#[spec]
fn empty(count: Count) -> Count {
    count
}

fn main() {}
