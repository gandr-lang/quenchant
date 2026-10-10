//! Exercise expansion in a real consumer crate rather than calling the host
//! API.

#![cfg_attr(not(anodized_print), no_std)]
#![cfg_attr(
    dylint_lib = "quenchant_dylints",
    deny(
        spec_attribute_present,
        adequacy_present,
        maybe_shape,
        erased_error_signature,
        spec_attribute_unqualified
    )
)]

extern crate alloc;
#[cfg(any(anodized_panic, anodized_print))]
extern crate anodized as backend;
extern crate self as anodized;
extern crate std;

#[cfg(any(anodized_panic, anodized_print))]
pub use anodized_macros::spec as __instrument;
#[cfg(any(anodized_panic, anodized_print))]
pub use backend::__;
#[cfg(any(anodized_panic, anodized_print))]
pub use backend::result;
#[cfg(any(anodized_panic, anodized_print))]
pub use backend::types;
pub use quenchant_spec_macros::__erase;
pub use quenchant_spec_macros::spec;

#[cfg(test)]
mod tests
{
    #[cfg(anodized_panic)]
    use alloc::string::String;
    #[cfg(not(anodized_panic))]
    use alloc::string::ToString as _;

    use crate::spec;
    /// A result whose two variants distinguish ordinary code from specification
    /// checks.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Receipt
    {
        /// The specification accepts this result.
        Preserved,
        /// Ordinary code can return this result, which the specification
        /// rejects.
        Altered,
    }

    #[cfg(not(any(anodized_panic, anodized_print)))]
    #[spec(ensures: false)]
    /// Preserve nested ordinary declarations and macro-language data without
    /// instrumentation.
    ///
    /// # Specification
    /// - ensures: retains both macro matcher arms and every nested function
    ///   body.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — nested bodies and both macro arms preserve their
    ///   distinct results.
    /// - witness: `expansion::tests::disabled_preserves_nested_code_and_macro_languages`
    fn without_instrumentation() -> (Receipt, Receipt, Receipt, Receipt)
    {
        #[spec(ensures: false)]
        /// A false predicate cannot remove the ordinary nested body in this
        /// mode.
        ///
        /// # Specification
        /// - ensures: returns the semantic marker.
        /// - panics: none.
        ///
        /// # Adequacy
        /// - hypothesis: L3 — the nested body returns the preserved marker
        ///   without enforcing.
        /// - witness: `expansion::tests::disabled_preserves_nested_code_and_macro_languages`
        fn nested() -> Receipt
        {
            Receipt::Preserved
        }

        macro_rules! classify {
            (#[spec]) => {
                Receipt::Preserved
            };
            () => {
                Receipt::Altered
            };
        }
        macro_rules! retained_payload {
        () => { classify!(#[spec]) };
    }
        (
            nested(),
            classify!(#[spec]),
            retained_payload!(),
            classify!(),
        )
    }

    #[cfg(not(any(anodized_panic, anodized_print)))]
    #[test]
    fn disabled_preserves_nested_code_and_macro_languages()
    {
        assert_eq!(
            without_instrumentation(),
            (
                Receipt::Preserved,
                Receipt::Preserved,
                Receipt::Preserved,
                Receipt::Altered
            ),
        );
    }

    #[cfg(not(any(anodized_panic, anodized_print)))]
    #[spec(maintains: unavailable_type_predicate(self))]
    /// A type predicate is erased without resolving its names.
    #[repr(transparent)]
    struct Stripped(Receipt);

    #[cfg(not(any(anodized_panic, anodized_print)))]
    #[spec(maintains: unavailable_enum_predicate(self))]
    /// Enum variants remain ordinary data in strip mode.
    enum StrippedChoice
    {
        /// Carry an unchanged payload.
        Value(Receipt),
    }

    #[cfg(not(any(anodized_panic, anodized_print)))]
    #[spec(ensures: unavailable_const_predicate())]
    /// Const evaluation must not resolve an erased predicate.
    ///
    /// # Specification
    /// - ensures: returns the supplied receipt at compile time and runtime.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — both receipts survive runtime and const evaluation.
    /// - witness: `expansion::tests::disabled_preserves_types_and_const_evaluation`
    const fn stripped_const(receipt: Receipt) -> Receipt
    {
        receipt
    }

    #[cfg(not(any(anodized_panic, anodized_print)))]
    #[test]
    fn disabled_preserves_types_and_const_evaluation()
    {
        const RECEIPT: Receipt = stripped_const(Receipt::Preserved);
        let StrippedChoice::Value(receipt) = StrippedChoice::Value(RECEIPT);
        assert_eq!(Stripped(receipt).0, Receipt::Preserved);
        assert_eq!(stripped_const(Receipt::Altered), Receipt::Altered);
    }

    #[cfg_attr(
        any(anodized_panic, anodized_print),
        expect(
            non_upper_case_globals,
            reason = "The backend emits lowercase associated qualifier constants."
        )
    )]
    #[spec]
    /// A nested method obligation belongs to the enclosing trait annotation.
    trait Inspect
    {
        #[spec(ensures: |output| output == Receipt::Preserved)]
        /// Expose the receiver's result for the enclosing specification to
        /// inspect.
        ///
        /// # Specification
        /// - ensures: enforcing instrumentation admits only the preserved
        ///   result.
        /// - panics: the backend rejects a violated postcondition when
        ///   enforcing.
        ///
        /// # Adequacy
        /// - hypothesis: L3 — the altered receipt fails only under enforcement.
        /// - witness: `expansion::tests::nested_trait_obligations_follow_the_selected_mode`
        fn inspect(&self) -> Receipt;
    }

    #[spec]
    impl Inspect for Receipt
    {
        /// Keep the ordinary result independent of instrumentation mode.
        ///
        /// # Specification
        /// - ensures: the uninstrumented body preserves either semantic
        ///   variant.
        /// - panics: only through enforcing specification instrumentation.
        ///
        /// # Adequacy
        /// - hypothesis: L3 — both receiver variants retain their value without
        ///   enforcement.
        /// - witness: `expansion::tests::nested_trait_obligations_follow_the_selected_mode`
        #[spec(ensures: |output| output == *self)]
        fn inspect(&self) -> Self
        {
            *self
        }
    }

    #[test]
    fn nested_trait_obligations_follow_the_selected_mode()
    {
        assert_eq!(Receipt::Preserved.inspect(), Receipt::Preserved);
        #[cfg(anodized_panic)]
        {
            let failure = std::panic::catch_unwind(|| Receipt::Altered.inspect())
                .expect_err("the inherited postcondition must reject the altered result");
            assert_postcondition_failure(failure.as_ref());
        }
        #[cfg(not(anodized_panic))]
        assert_eq!(Receipt::Altered.inspect(), Receipt::Altered);
    }

    #[spec(ensures: |ref output| output.is_ok())]
    /// Let the backend inspect an early error return without consuming its
    /// move-only payload.
    ///
    /// # Specification
    /// - ensures: enforcing instrumentation rejects an error return.
    /// - panics: the backend rejects the violated postcondition.
    ///
    /// # Errors
    /// Without enforcement, the input's I/O error is propagated unchanged.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — early errors survive erasure and fail enforcing
    ///   postconditions.
    /// - witness: `expansion::tests::early_return_postcondition_follows_the_selected_mode`
    fn checked_exit(value: Result<Receipt, std::io::Error>) -> Result<Receipt, std::io::Error>
    {
        let value = value?;
        Ok(value)
    }

    #[cfg(anodized_panic)]
    /// Reject unrelated panics as evidence of specification enforcement.
    ///
    /// # Specification
    /// - ensures: requires the backend's postcondition failure diagnostic.
    /// - panics: an unrelated payload fails this test helper.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the violated postconditions report a backend failure,
    ///   not a body panic.
    /// - witness: `expansion::tests::nested_trait_obligations_follow_the_selected_mode`
    /// - witness: `expansion::tests::early_return_postcondition_follows_the_selected_mode`
    #[spec(ensures: {
        let text = failure.downcast_ref::<String>().map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied());
        text.is_some_and(|text| text.contains("postcondition failed"))
    })]
    fn assert_postcondition_failure(failure: &(dyn core::any::Any + Send))
    {
        let text = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .expect("the backend panic carries text");
        assert!(
            text.contains("postcondition failed"),
            "wrong failure: {text}"
        );
    }

    #[test]
    fn early_return_postcondition_follows_the_selected_mode()
    {
        #[cfg(anodized_panic)]
        {
            let failure =
                std::panic::catch_unwind(|| checked_exit(Err(std::io::Error::other("sentinel"))))
                    .expect_err("the postcondition must reject the early error return");
            assert_postcondition_failure(failure.as_ref());
        }
        #[cfg(not(anodized_panic))]
        {
            let result = checked_exit(Err(std::io::Error::other("sentinel")));
            assert_eq!(
                result
                    .expect_err("the body still returns its error")
                    .to_string(),
                "sentinel"
            );
        }
        assert_eq!(
            checked_exit(Ok(Receipt::Preserved)).expect("valid result"),
            Receipt::Preserved
        );
    }
}
