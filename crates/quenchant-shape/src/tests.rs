//! Consumer-visible absence transitions, callback behavior, and wrapper
//! specifications.

use core::cell::Cell;

use super::Maybe;
use super::absence_query;

reason_enum! {
    /// Absence during this lookup's two stages.
    pub mod lookup {
        /// Why this lookup did not provide a value.
        #[derive(Debug, Eq, PartialEq)]
        pub enum Unavailable {
            /// The key was searched for and not found.
            Missing,
            /// The stage that searches this key has not run.
            Unsearched,
        }
    }
}

/// Construct absence through this site's closed generic reason bound.
///
/// # Specification
/// - requires: `Reason` implements this lookup site's sealed reason trait, so
///   the bound admits the site's own enum and nothing else.
/// - ensures: returns the absent arm carrying `reason` unchanged.
/// - provides: absence reached through the sealed bound rather than the
///   concrete enum, which is what the compile-fail example on `reason_enum!`
///   denies to an outside implementer.
/// - panics: none.
#[quenchant::spec(ensures: |ref output| matches!(output, Maybe::Absent(_)))]
fn unavailable<Reason>(reason: Reason) -> Maybe<Ticket, Reason>
where
    Reason: lookup::Reason,
{
    Maybe::Absent(reason)
}

/// A move-only input: combinators must not require Clone or Copy.
#[derive(Debug, Eq, PartialEq)]
enum Ticket
{
    /// First route.
    North,
    /// Second route.
    South,
}

/// A move-only closure capture with a different output type.
#[derive(Debug, Eq, PartialEq)]
enum Receipt
{
    /// The next stage accepted the ticket.
    Accepted,
}

/// Callback state without an untyped boolean or arithmetic counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Visit
{
    /// The callback has not run.
    Untouched,
    /// The callback ran.
    Visited,
}

#[test]
fn mapping_preserves_absence_and_moves_values()
{
    let visits = Cell::new(Visit::Untouched);
    let observed = &visits;
    let receipt = Receipt::Accepted;
    let present: Maybe<Ticket, lookup::Unavailable> = Maybe::Present(Ticket::North);
    let mapped = present.map(move |_ticket| {
        observed.set(Visit::Visited);
        receipt
    });
    assert_eq!(mapped, Maybe::Present(Receipt::Accepted));
    assert_eq!(visits.get(), Visit::Visited);

    visits.set(Visit::Untouched);
    let receipt = Receipt::Accepted;
    let absent = unavailable(lookup::Unavailable::Unsearched);
    let mapped = absent.map(move |_ticket| {
        observed.set(Visit::Visited);
        receipt
    });
    assert_eq!(mapped, Maybe::Absent(lookup::Unavailable::Unsearched));
    assert_eq!(visits.get(), Visit::Untouched);
}

#[test]
fn chaining_preserves_both_absence_transitions()
{
    let present: Maybe<Ticket, lookup::Unavailable> = Maybe::Present(Ticket::North);
    let receipt = Receipt::Accepted;
    assert_eq!(
        present.and_then(move |_ticket| Maybe::Present(receipt)),
        Maybe::Present(Receipt::Accepted)
    );

    let present: Maybe<Ticket, lookup::Unavailable> = Maybe::Present(Ticket::South);
    let produced: Maybe<Receipt, lookup::Unavailable> =
        present.and_then(|_ticket| Maybe::Absent(lookup::Unavailable::Unsearched));
    assert_eq!(produced, Maybe::Absent(lookup::Unavailable::Unsearched));

    let visits = Cell::new(Visit::Untouched);
    let absent = unavailable(lookup::Unavailable::Missing);
    let preserved: Maybe<Receipt, lookup::Unavailable> = absent.and_then(|_ticket| {
        visits.set(Visit::Visited);
        Maybe::Absent(lookup::Unavailable::Unsearched)
    });
    assert_eq!(preserved, Maybe::Absent(lookup::Unavailable::Missing));
    assert_eq!(visits.get(), Visit::Untouched);
}

#[test]
fn reason_query_preserves_the_original_value()
{
    let absent: Maybe<Ticket, lookup::Unavailable> = Maybe::Absent(lookup::Unavailable::Missing);
    assert_eq!(
        absent.absent_reason(),
        Maybe::Present(&lookup::Unavailable::Missing)
    );
    assert_eq!(absent, Maybe::Absent(lookup::Unavailable::Missing));

    let present: Maybe<Ticket, lookup::Unavailable> = Maybe::Present(Ticket::South);
    assert_eq!(
        present.absent_reason(),
        Maybe::Absent(absence_query::ValuePresent::Present)
    );
    assert_eq!(
        present.map(|_ticket| Receipt::Accepted),
        Maybe::Present(Receipt::Accepted)
    );
}

/// Failure at the boundary that requires a completed lookup.
#[derive(Debug, Eq, PartialEq)]
enum LookupFailure
{
    /// A required key is missing.
    MissingKey,
    /// A required search has not run.
    SearchRequired,
}

impl core::fmt::Display for LookupFailure
{
    /// Write the fixed message this failure variant names.
    ///
    /// # Specification
    /// - ensures: writes one fixed message per variant, selected by the variant
    ///   alone, and writes nothing else.
    /// - provides: the display face `core::error::Error` requires of this
    ///   boundary failure. The predicate is empty: the text written reaches the
    ///   formatter's sink rather than the return value, so no declared
    ///   projection observes it.
    /// - fails: propagates the formatter's own write failure unchanged; this
    ///   method originates none.
    /// - panics: none.
    ///
    /// # Errors
    /// - `core::fmt::Error`: the formatter's sink refused the write.
    #[quenchant::spec]
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result
    {
        f.write_str(match *self {
            | Self::MissingKey => "required key is missing",
            | Self::SearchRequired => "search is required",
        })
    }
}

impl core::error::Error for LookupFailure
{
}

#[test]
fn failure_promotion_is_explicit_and_reason_sensitive()
{
    let visits = Cell::new(Visit::Untouched);
    let present: Maybe<Ticket, lookup::Unavailable> = Maybe::Present(Ticket::North);
    let result = present.into_result(|_reason| {
        visits.set(Visit::Visited);
        LookupFailure::MissingKey
    });
    assert!(matches!(result, Ok(Ticket::North)));
    assert_eq!(visits.get(), Visit::Untouched);

    for (reason, failure) in [
        (lookup::Unavailable::Missing, LookupFailure::MissingKey),
        (
            lookup::Unavailable::Unsearched,
            LookupFailure::SearchRequired,
        ),
    ] {
        visits.set(Visit::Untouched);
        let absent: Maybe<Ticket, lookup::Unavailable> = Maybe::Absent(reason);
        let result = absent.into_result(|reason| {
            visits.set(Visit::Visited);
            match reason {
                | lookup::Unavailable::Missing => LookupFailure::MissingKey,
                | lookup::Unavailable::Unsearched => LookupFailure::SearchRequired,
            }
        });
        assert_eq!(result, Err(failure));
        assert_eq!(visits.get(), Visit::Visited);
    }
}

nominal_type! {
    /// Permission combined by union rather than primitive addition.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Permission(bool);
}

delegate_ops!(binary Permission, Add::add => core::ops::BitOr::bitor);
delegate_ops!(unary Permission, Not::not => core::ops::Not::not);
delegate_ops!(assign Permission, AddAssign::add_assign => core::ops::BitOrAssign::bitor_assign);

#[test]
fn selected_operators_preserve_the_permission_domain()
{
    assert_eq!(Permission(false) + Permission(true), Permission(true));
    assert_eq!(Permission(false) + Permission(false), Permission(false));
    assert_eq!(!Permission(true), Permission(false));
    assert_eq!(!Permission(false), Permission(true));
    let mut permission = Permission(false);
    permission += Permission(true);
    assert_eq!(permission, Permission(true));
    permission += Permission(false);
    assert_eq!(permission, Permission(true));
}

#[test]
fn nominal_wrapper_preserves_transparent_layout()
{
    assert_eq!(
        core::mem::size_of::<Permission>(),
        core::mem::size_of::<bool>()
    );
    assert_eq!(
        core::mem::align_of::<Permission>(),
        core::mem::align_of::<bool>()
    );
}

/// Reject a false postcondition only when the proc-macro dependency enforces
/// it.
#[cfg(all(feature = "anodized", anodized_panic))]
#[test]
fn specification_enforcement_rejects_false_postcondition()
{
    /// Exercise postcondition enforcement without any body-originated panic.
    ///
    /// # Specification
    /// - ensures: false, deliberately violated to witness enforcement.
    /// - panics: the enforcing postcondition rejects every return.
    ///
    /// # Adequacy
    /// - hypothesis: L3 an empty body cannot produce the asserted postcondition
    ///   failure; missing proc-macro enforcement returns normally instead.
    /// - witness: `shape::tests::specification_enforcement_rejects_false_postcondition`
    #[quenchant::spec(ensures: false)]
    fn rejected()
    {
    }

    let failure = std::panic::catch_unwind(rejected)
        .expect_err("the enforcing lane must reject a false postcondition");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("the backend reports a textual failure");
    assert!(
        message.starts_with("postcondition failed"),
        "unrelated panic: {message}"
    );
}
