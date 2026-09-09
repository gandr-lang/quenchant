#![allow(dead_code, specification_present)]

/// The direction of the checking judgement.
///
/// # Judgement
/// - direction: the two modes, forced by the term's syntactic class.
enum Direction
{
    Synthesises,
    Checks,
}

/// The head of a type, which no declaration marks.
enum TypeHead
{
    Arrow,
    Base,
}

/// What a rule answers.
enum Verdict
{
    Accepted,
    Refused,
}

/// Every direction named in an arm of its own.
fn exhaustive(direction: Direction) -> Verdict
{
    match direction {
        Direction::Checks => Verdict::Accepted,
        Direction::Synthesises => Verdict::Refused,
    }
}

/// A binding carrying a subpattern names its case, so it is not a fallback.
fn bound_case(direction: Direction) -> Direction
{
    match direction {
        checked @ Direction::Checks => checked,
        Direction::Synthesises => Direction::Synthesises,
    }
}

/// A `matches!` carries a fallback arm the author never wrote, and asks a
/// boolean question rather than choosing a mode.
fn boolean_test(direction: Direction) -> Verdict
{
    if matches!(direction, Direction::Checks) {
        Verdict::Accepted
    } else {
        Verdict::Refused
    }
}

/// A wildcard arm over the judgement's direction.
fn wildcard_arm(direction: Direction) -> Verdict
{
    match direction {
        Direction::Checks => Verdict::Accepted,
        _ => Verdict::Refused,
    }
}

/// A bare binding over the judgement's direction.
fn binding_arm(direction: Direction) -> Direction
{
    match direction {
        Direction::Checks => Direction::Checks,
        other => other,
    }
}

/// A fallback hidden among the named alternatives of an or-pattern.
fn or_alternative(direction: Direction) -> Verdict
{
    match direction {
        Direction::Checks | _ => Verdict::Accepted,
    }
}

/// A guard does not turn a fallback arm into a named case.
fn guarded_fallback(direction: Direction, verdict: Verdict) -> Verdict
{
    match direction {
        Direction::Checks => Verdict::Accepted,
        _ if matches!(verdict, Verdict::Refused) => Verdict::Refused,
        Direction::Synthesises => Verdict::Accepted,
    }
}

/// The direction reached through a reference is still the direction.
fn through_a_reference(direction: &Direction) -> Verdict
{
    match direction {
        Direction::Checks => Verdict::Accepted,
        _ => Verdict::Refused,
    }
}

/// A tuple carrying the direction is not the direction: the plane is a
/// type-identity test on the whole scrutinee. The miss is deliberate and its
/// silence in `mode_dispatch.stderr` is the record.
fn tupled_scrutinee(direction: Direction, head: TypeHead) -> Verdict
{
    match (direction, head) {
        (Direction::Checks, TypeHead::Arrow) => Verdict::Accepted,
        _ => Verdict::Refused,
    }
}

/// `if let` has no arms, so its else branch is outside this gate's reach. The
/// miss is deliberate and its silence in `mode_dispatch.stderr` is the record.
fn if_let_fallback(direction: Direction) -> Verdict
{
    if let Direction::Checks = direction {
        Verdict::Accepted
    } else {
        Verdict::Refused
    }
}

/// A verdict for a type head, on a face declaring nothing.
fn head_verdict(head: TypeHead) -> Verdict
{
    match head {
        TypeHead::Arrow => Verdict::Accepted,
        TypeHead::Base => Verdict::Refused,
    }
}

/// A match on a type no declaration marks, fallback arm and all.
fn unmarked(head: TypeHead) -> Verdict
{
    match head {
        TypeHead::Arrow => Verdict::Accepted,
        _ => Verdict::Refused,
    }
}

/// The head a type is built from.
fn head_of(head: TypeHead) -> TypeHead
{
    head
}

/// A face reading its mode off the type the term is checked against.
///
/// # Judgement
/// - expected: `expected`
fn check_against(expected: TypeHead) -> Verdict
{
    match expected {
        TypeHead::Arrow => Verdict::Accepted,
        _ => Verdict::Refused,
    }
}

/// The expected type reached beneath a call is still the expected type.
///
/// # Judgement
/// - expected: `expected`
fn check_beneath_a_call(expected: TypeHead) -> Verdict
{
    match head_of(expected) {
        TypeHead::Arrow => Verdict::Accepted,
        other => head_verdict(other),
    }
}

/// A match on a parameter the declaration does not name, with the expected type
/// used inside an arm rather than dispatched on.
///
/// # Judgement
/// - expected: `expected`
fn other_parameter(expected: TypeHead, offered: TypeHead) -> Verdict
{
    match offered {
        TypeHead::Arrow => head_verdict(expected),
        _ => Verdict::Refused,
    }
}

/// A section stating neither of the two bullets.
///
/// # Judgement
/// - mode: whichever the term asks for.
enum SaysNothing
{
    Only,
}

/// A direction bullet stating no value.
///
/// # Judgement
/// - direction:
enum Unexplained
{
    Only,
}

/// An expected bullet whose value is prose rather than one backticked name.
///
/// # Judgement
/// - expected: expected
fn malformed_expected(head: TypeHead) -> Verdict
{
    head_verdict(head)
}

/// An expected bullet naming a parameter this face does not take.
///
/// # Judgement
/// - expected: `offered`
fn unbound_expected(head: TypeHead) -> Verdict
{
    head_verdict(head)
}

/// A direction bullet on the face rather than on the type.
///
/// # Judgement
/// - direction: the mode this face runs in.
fn direction_off_type(head: TypeHead) -> Verdict
{
    head_verdict(head)
}

/// An expected bullet on a type definition rather than on a face.
///
/// # Judgement
/// - expected: `expected`
enum ExpectedOffFunction
{
    Only,
}

/// A section on an item that carries no scrutinee at all.
///
/// # Judgement
/// - direction: nothing here dispatches on anything.
const MISPLACED: Verdict = Verdict::Accepted;

/// A judgement whose faces are trait methods.
trait Judgement
{
    /// An associated type carries no scrutinee.
    ///
    /// # Judgement
    /// - direction: an associated type is not a type definition.
    type Answer;

    /// A required method has no body, so nothing it names can gate a match, and
    /// an implementation's parameters are the implementation's own.
    ///
    /// # Judgement
    /// - expected: `expected`
    fn check_declared(&self, expected: TypeHead) -> Verdict;

    /// A provided method has a body, so its declaration is read where the
    /// parameters it names are in reach.
    ///
    /// # Judgement
    /// - expected: `expected`
    fn check_provided(&self, expected: TypeHead) -> Verdict
    {
        match expected {
            TypeHead::Arrow => Verdict::Accepted,
            _ => Verdict::Refused,
        }
    }
}

/// The face the judgement's implementations hang on.
struct Face;

impl Face
{
    /// An associated const carries no scrutinee.
    ///
    /// # Judgement
    /// - direction: an associated const is not a type definition.
    const FALLBACK: Verdict = Verdict::Refused;
}

impl Judgement for Face
{
    type Answer = Verdict;

    /// An associated function has a body, so the declaration marks its
    /// parameter exactly as on a free function.
    ///
    /// # Judgement
    /// - expected: `expected`
    fn check_declared(&self, expected: TypeHead) -> Verdict
    {
        match expected {
            TypeHead::Arrow => Verdict::Accepted,
            _ => Self::FALLBACK,
        }
    }
}

extern "C"
{
    /// A foreign function has no body and no scrutinee.
    ///
    /// # Judgement
    /// - expected: `expected`
    fn check_foreign();
}

fn main()
{
}
