//! Executed postcondition failures distinguish enforcement from compile-only
//! acceptance.

use core::future::Future as _;

/// Nominal output keeps the witness inside the signature policy.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Count(u32);

/// Body-level refusal remains distinct from an instrumentation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Refused;

/// Question-mark propagation still reaches the function's postcondition
/// boundary.
///
/// # Specification
/// - ensures: the output succeeds; deliberately falsified by a refused input.
/// - panics: enforcing builds panic on the refused output.
///
/// # Adequacy
/// - hypothesis: L3 — the same failure reaches postconditions through `?` and
///   explicit return, and the success control preserves its exact payload.
/// - witness: `specification_tests::early_exits_reach_postconditions`
#[quenchant::spec(ensures: |output| output.is_ok())]
fn question(value: Result<Count, Refused>) -> Result<Count, Refused>
{
    let value = value?;
    Ok(value)
}

/// Explicit early return still reaches the function's postcondition boundary.
///
/// # Specification
/// - ensures: the output succeeds; deliberately falsified by a refused input.
/// - panics: enforcing builds panic on the refused output.
///
/// # Adequacy
/// - hypothesis: L3 — explicit return is the control for the `?` exit.
/// - witness: `specification_tests::early_exits_reach_postconditions`
#[quenchant::spec(ensures: |output| output.is_ok())]
fn explicit(value: Result<Count, Refused>) -> Result<Count, Refused>
{
    let Ok(value) = value
    else {
        return Err(Refused);
    };
    Ok(value)
}

/// Async early completion remains observable by the enclosing postcondition.
///
/// # Specification
/// - ensures: the output succeeds; deliberately falsified by a refused input.
/// - panics: enforcing builds panic on the refused output when polled.
///
/// # Adequacy
/// - hypothesis: L3 — the awaited generated closure cannot bypass the check.
/// - witness: `specification_tests::async_early_exit_reaches_postcondition`
#[quenchant::spec(ensures: |output| output.is_ok())]
async fn asynchronous(value: Result<Count, Refused>) -> Result<Count, Refused>
{
    let value = core::future::ready(value).await?;
    Ok(value)
}

/// The failure observer distinguishes instrumentation from unrelated body
/// panics.
///
/// # Specification
/// - ensures: enforcing invocations reject exactly the deliberate postcondition
///   failure; non-enforcing invocations preserve the body's typed refusal.
/// - panics: any mismatch fails the test.
///
/// # Adequacy
/// - hypothesis: L3 — synchronous and async callers exercise both mode
///   branches.
/// - witness: `specification_tests::early_exits_reach_postconditions`
/// - witness: `specification_tests::async_early_exit_reaches_postcondition`
fn assert_failure(outcome: std::thread::Result<Result<Count, Refused>>)
{
    if cfg!(all(feature = "anodized", anodized_panic)) && !cfg!(anodized_discard_specs) {
        let panic = outcome.expect_err("the enforcing lane must reject this postcondition");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .expect("the specification backend reports a textual failure");
        assert!(
            message.starts_with("postcondition failed"),
            "unrelated panic: {message}"
        );
    }
    else {
        assert_eq!(
            outcome.expect("non-enforcing modes must preserve the body's result"),
            Err(Refused)
        );
    }
}

#[test]
fn early_exits_reach_postconditions()
{
    for function in [question, explicit] {
        assert_eq!(function(Ok(Count(7))), Ok(Count(7)));
        assert_failure(std::panic::catch_unwind(|| function(Err(Refused))));
    }
}

#[test]
fn async_early_exit_reaches_postcondition()
{
    let outcome = std::panic::catch_unwind(|| {
        let mut future = core::pin::pin!(asynchronous(Err(Refused)));
        let mut context = core::task::Context::from_waker(core::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            | core::task::Poll::Ready(value) => value,
            | core::task::Poll::Pending => panic!("this body has no suspension point"),
        }
    });
    assert_failure(outcome);
}
