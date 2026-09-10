#![allow(dead_code, specification_present)]

/// An item carrying no adequacy block at all.
///
/// Presence is a review obligation, not a lint one, so nothing is reported
/// here.
///
/// # Specification
/// - panics: none.
fn no_block()
{
}

/// A block that states its rung and names its witness.
///
/// # Adequacy
/// - hypothesis: L3 — the boundary inputs are enumerated exhaustively and each
///   is paired with an exact-variant assertion.
/// - witness: `tests::the_empty_input_is_refused`
/// - witness: `acceptance::the_two_walks_agree`
fn accepted_block()
{
}

/// A block whose rung is named on a continuation line, and whose section is
/// terminated by a subheading before the injected bullets below it.
///
/// # Adequacy
/// - hypothesis: the plan continues onto a second line, where the rung
///   L2 is finally named.
/// - witness: `oracle::agrees_with_the_reference`
///
/// ## Specifications
/// - hypothesis: injected by an attribute macro, below a subheading.
/// - witness: injected by an attribute macro, below a subheading.
fn terminated_section()
{
}

/// A block naming witnesses but stating no hypothesis.
///
/// # Adequacy
/// - witness: `tests::the_empty_input_is_refused`
fn missing_hypothesis()
{
}

/// A block whose hypothesis names no rung of the ladder.
///
/// # Adequacy
/// - hypothesis: the boundary cases are covered.
/// - witness: `tests::the_empty_input_is_refused`
fn hypothesis_without_a_rung()
{
}

/// A block stating a plan and naming no evidence for it.
///
/// # Adequacy
/// - hypothesis: L1 — the returned certificate is validated against the input.
fn missing_witness()
{
}

/// A block stating two plans.
///
/// # Adequacy
/// - hypothesis: L1 — the returned certificate is validated against the input.
/// - hypothesis: L3 — and the boundary inputs are enumerated.
/// - witness: `tests::the_empty_input_is_refused`
fn duplicate_hypothesis()
{
}

/// A block whose evidence precedes its plan.
///
/// # Adequacy
/// - witness: `tests::the_empty_input_is_refused`
/// - hypothesis: L3 — the boundary inputs are enumerated.
fn hypothesis_after_the_evidence()
{
}

/// A block whose witness is prose rather than an exact path.
///
/// # Adequacy
/// - hypothesis: L3 — the boundary inputs are enumerated.
/// - witness: the unit tests below cover this.
fn malformed_witness()
{
}

/// A block claiming the declaration-only exemption on an item with a body.
///
/// # Adequacy
/// - hypothesis: L0 — illegal states are unrepresentable.
/// - declaration-only: there is nothing here to witness.
fn exemption_on_a_body()
{
}

trait Seam
{
    /// A required trait method taking the reasoned exemption.
    ///
    /// # Adequacy
    /// - hypothesis: L0 — the declaration fixes the shape and has no behaviour
    ///   of its own.
    /// - declaration-only: every implementation is witnessed at its own impl,
    ///   and the declaration has no body a mutant could change.
    fn accepted_declaration(&self);

    /// A required trait method taking the exemption without a reason.
    ///
    /// # Adequacy
    /// - hypothesis: L0 — the declaration fixes the shape.
    /// - declaration-only:
    fn unreasoned_declaration(&self);

    /// A required trait method combining the exemption with a witness.
    ///
    /// # Adequacy
    /// - hypothesis: L0 — the declaration fixes the shape.
    /// - declaration-only: every implementation is witnessed at its own impl.
    /// - witness: `tests::the_empty_input_is_refused`
    fn exemption_and_witness(&self);

    /// A provided trait method taking the exemption it has no claim to.
    ///
    /// # Adequacy
    /// - hypothesis: L0 — the default body is trivial.
    /// - declaration-only: the default body is trivial.
    fn provided_method(&self)
    {
    }
}

fn main()
{
}
