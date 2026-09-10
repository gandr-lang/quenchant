#![allow(dead_code, specification_present)]
#![allow(unconditional_recursion)]

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Count(u32);

// No exception attribute at all: every member of the cycle is denied.

fn unguarded_direct(value: Count) -> Count
{
    unguarded_direct(value)
}

fn unguarded_mutual_left(value: Count) -> Count
{
    unguarded_mutual_right(value)
}

fn unguarded_mutual_right(value: Count) -> Count
{
    unguarded_mutual_left(value)
}

// An approved exception with a complete, unrefuted section: accepted.

enum Fuel
{
    One,
    Zero,
}

fn bounded_entry() -> Count
{
    bounded_with_fuel(Fuel::One)
}

/// Count down a closed fuel enum rather than any caller-supplied value.
///
/// # Termination
/// - reason: the recursion consumes the private closed `Fuel` enum, not input.
/// - measure: remaining `Fuel` variants before `Zero`.
/// - boundedness: `One` strictly recurses to `Zero`, and `Zero` returns.
/// - input recursion: none.
#[expect(
    recursion_forbidden,
    reason = "owner-approved: the recursion is over a closed two-variant enum, never over input"
)]
fn bounded_with_fuel(fuel: Fuel) -> Count
{
    match fuel {
        | Fuel::One => bounded_with_fuel(Fuel::Zero),
        | Fuel::Zero => Count(0),
    }
}

/// Descend the input value, and say so.
///
/// # Termination
/// - reason: structural descent is the natural control shape for this value.
/// - measure: remaining structure of the input wrapper.
/// - boundedness: the input is a finite tree.
/// - input recursion: structural descent over the input value.
#[expect(
    recursion_forbidden,
    reason = "owner-approved: the input recursion is declared rather than denied"
)]
fn declared_input_recursion(value: Count) -> Count
{
    declared_input_recursion(value)
}

/// Wrap the bullet values across source lines.
///
/// # Termination
/// - reason: the reason bullet wraps onto a continuation line, which the
///   section reader folds back into the bullet above it.
/// - measure: remaining `Fuel` variants before `Zero`.
/// - boundedness: `One` strictly recurses to `Zero`, and `Zero` returns.
/// - input recursion: none.
#[expect(
    recursion_forbidden,
    reason = "owner-approved: a wrapped bullet value is still one bullet"
)]
fn wrapped_bullets(fuel: Fuel) -> Count
{
    match fuel {
        | Fuel::One => wrapped_bullets(Fuel::Zero),
        | Fuel::Zero => Count(0),
    }
}

/// Justify the input recursion across a wrapped bullet value.
///
/// # Termination
/// - reason: the input recursion is declared, and its justification wraps
///   across several source lines the way rustfmt reflows doc comments.
/// - measure: remaining structure of the input wrapper.
/// - boundedness: the input is a finite tree.
/// - input recursion: structural descent over the input value, which shrinks
///   at every edge because each call takes a strict subterm of its argument
///   rather than the argument itself.
#[expect(
    recursion_forbidden,
    reason = "owner-approved: a wrapped justification is still one bullet value"
)]
fn multiline_justified_input_recursion(value: Count) -> Count
{
    multiline_justified_input_recursion(value)
}

// An exception claimed without a justification: denied.

#[expect(recursion_forbidden, reason = "owner-approved but undocumented")]
fn missing_section(value: Count) -> Count
{
    missing_section(value)
}

/// # Termination
/// - reason: the measure and boundedness bullets are absent.
#[expect(recursion_forbidden, reason = "owner-approved but incomplete")]
fn short_section(value: Count) -> Count
{
    short_section(value)
}

/// # Termination
/// - measure: remaining wrapper depth.
/// - reason: the bullets are out of the order the grammar fixes.
/// - boundedness: each recursive call descends.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "owner-approved but out of order")]
fn misordered_section(value: Count) -> Count
{
    misordered_section(value)
}

/// The section ends at the next heading, so the bullets below do not complete
/// it — this is the shape an attribute macro's injected doc block produces.
///
/// # Termination
/// - reason: the section ends at the following heading.
///
/// # Specifications
/// - measure: injected below a further heading, not part of the section.
/// - boundedness: injected below a further heading.
/// - input recursion: none.
#[expect(
    recursion_forbidden,
    reason = "owner-approved but truncated by an injected doc block"
)]
fn heading_terminated_section(value: Count) -> Count
{
    heading_terminated_section(value)
}

#[expect(recursion_forbidden)]
fn unreasoned_exception(value: Count) -> Count
{
    unreasoned_exception(value)
}

// Complete sections whose `- input recursion: none.` claim the call graph
// refutes: denied.

/// # Termination
/// - reason: direct recursion falsely claims not to carry caller input.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
fn false_none_direct(value: Count) -> Count
{
    false_none_direct(value)
}

/// # Termination
/// - reason: let-derived recursion falsely claims not to carry caller input.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
fn false_none_let(value: Count) -> Count
{
    let child = value;
    false_none_let(child)
}

/// # Termination
/// - reason: match-derived recursion falsely claims not to carry caller input.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
fn false_none_match(value: Option<Count>) -> Count
{
    match value {
        | Some(child) => false_none_match(Some(child)),
        | None => Count(0),
    }
}

/// # Termination
/// - reason: mutual recursion falsely claims not to carry caller input.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
fn false_none_mutual_left(value: Count) -> Count
{
    false_none_mutual_right(value)
}

/// # Termination
/// - reason: mutual recursion falsely claims not to carry caller input.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none.
#[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
fn false_none_mutual_right(value: Count) -> Count
{
    false_none_mutual_left(value)
}

/// # Termination
/// - reason: trailing prose must not disarm the refutation.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none. The value is passed straight back, so this claim
///   is false and the prose after the claim must not hide it.
#[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
fn false_none_trailing_prose(value: Count) -> Count
{
    false_none_trailing_prose(value)
}

struct Receiver;

impl Receiver
{
    /// # Termination
    /// - reason: method recursion falsely claims not to carry caller input.
    /// - measure: finite receiver chain.
    /// - boundedness: each recursive call descends.
    /// - input recursion: none.
    #[expect(recursion_forbidden, reason = "owner-approved with a false claim")]
    fn false_none_self(&self)
    {
        self.false_none_self();
    }
}

// Recursion that reaches its callee through something other than a direct
// path: the call plane sees the first two and misses the third.

fn closure_mediated(value: Count) -> Count
{
    let step = || closure_mediated(value);
    step()
}

fn fn_item_indirection(value: Count) -> Count
{
    let call = fn_item_indirection;
    call(value)
}

// Recorded blind spot: coercion to a function pointer erases the callee's
// definition id, so no edge is recorded and this cycle is not denied.
fn fn_pointer_indirection(value: Count) -> Count
{
    let call: fn(Count) -> Count = fn_pointer_indirection;
    call(value)
}

// An expectation on an enclosing module is not an approval of the items inside
// it: the exception is owner-approved per item or not at all.

#[expect(
    recursion_forbidden,
    reason = "a module-level approval must not carry to the functions inside it"
)]
mod inherited_exception
{
    use super::Count;

    /// # Termination
    /// - reason: the section is complete, but nothing here approved the item.
    /// - measure: remaining `Count` depth.
    /// - boundedness: each recursive call descends.
    /// - input recursion: structural descent over the input value.
    pub fn borrowed_approval(value: Count) -> Count
    {
        borrowed_approval(value)
    }
}

// A `## `-level heading also terminates the section, so an injected block that
// nests under a subheading cannot complete a truncated one either.

/// # Termination
/// - reason: the section ends at the following subheading.
///
/// ## Specifications
/// - measure: injected below a subheading, not part of the section.
/// - boundedness: injected below a subheading.
/// - input recursion: structural descent, so the refutation cannot catch this
///   one and only the reader's terminator can.
#[expect(
    recursion_forbidden,
    reason = "owner-approved but truncated by an injected subheading block"
)]
fn subheading_terminated_section(value: Count) -> Count
{
    subheading_terminated_section(value)
}

/// # Termination
/// - reason: punctuation must not turn the claim into a declaration.
/// - measure: finite wrapper depth.
/// - boundedness: each recursive call descends.
/// - input recursion: none?
#[expect(recursion_forbidden, reason = "owner-approved with a punctuated false claim")]
fn false_none_punctuated(value: Count) -> Count
{
    false_none_punctuated(value)
}

// No cycle at all: accepted, and the section reader is never consulted.

fn acyclic(value: Count) -> Count
{
    value
}

fn main()
{
}
