// `?`, explicit return, and async exits remain inside the generated closure.
#![allow(dead_code, specification_present)]

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, PartialOrd)]
struct Count(u32);

impl Count {
    const ZERO: Self = Self(0);
}

#[derive(Clone, Copy, Debug)]
struct Refused;

/// Option early exits are valid beneath a postcondition.
#[quenchant::spec(ensures: |output| output.is_some())]
fn question_option(count: Option<Count>) -> Option<Count> {
    let count = count?;
    Some(count)
}

/// Result early exits are valid beneath a postcondition.
#[quenchant::spec(ensures: |output| output.is_ok())]
fn question_result(count: Result<Count, Refused>) -> Result<Count, Refused> {
    let count = count?;
    Ok(count)
}

/// A maintained predicate is checked after either exit path.
#[quenchant::spec(maintains: count != Some(Count::ZERO))]
fn maintained(count: Option<Count>) -> Option<Count> {
    let count = count?;
    Some(count)
}

/// Explicit returns return to the same generated checks.
#[quenchant::spec(ensures: |output| output.is_some())]
fn explicit_return(count: Option<Count>) -> Option<Count> {
    let Some(count) = count else { return None; };
    Some(count)
}

/// Async bodies run in an awaited closure before postconditions.
#[quenchant::spec(ensures: |output| output.is_some())]
async fn asynchronous(count: Option<Count>) -> Option<Count> {
    let count = count?;
    Some(count)
}

/// Nested closures remain legal and retain their own exit scope.
#[quenchant::spec(ensures: |output| output.is_some())]
fn nested(count: Option<Count>) -> Option<Count> {
    let get = || { let count = count?; Some(count) };
    get()
}

/// Built-in attributes remain compatible with precondition-only specs.
#[inline]
#[quenchant::spec(requires: count.is_some())]
fn precondition(count: Option<Count>) -> Option<Count> {
    let count = count?;
    Some(count)
}

fn main() {}
