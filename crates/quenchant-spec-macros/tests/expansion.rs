//! Exercise expansion in a real consumer crate rather than calling the host
//! API.

extern crate self as quenchant;

#[cfg(feature = "anodized")]
pub use anodized_macros::spec as __instrument;
pub use quenchant_spec_macros::__erase;
pub use quenchant_spec_macros::spec;

#[cfg(test)]
mod tests
{
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

    #[cfg(not(feature = "anodized"))]
    #[quenchant::spec(ensures: false)]
    /// Preserve nested ordinary declarations and macro-language data without
    /// instrumentation.
    ///
    /// # Specification
    /// - ensures: retains both macro matcher arms and every nested function
    ///   body.
    /// - panics: none.
    fn without_instrumentation() -> (Receipt, Receipt, Receipt, Receipt)
    {
        #[spec(ensures: false)]
        /// A false predicate cannot remove the ordinary nested body in this
        /// mode.
        ///
        /// # Specification
        /// - ensures: returns the semantic marker.
        /// - panics: none.
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

    #[cfg(not(feature = "anodized"))]
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

    #[cfg_attr(
        feature = "anodized",
        expect(
            non_upper_case_globals,
            reason = "The published backend emits lowercase associated qualifier constants."
        )
    )]
    #[quenchant::spec]
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
        fn inspect(&self) -> Receipt;
    }

    #[quenchant::spec]
    impl Inspect for Receipt
    {
        /// Keep the ordinary result independent of instrumentation mode.
        ///
        /// # Specification
        /// - ensures: the uninstrumented body preserves either semantic
        ///   variant.
        /// - panics: only through enforcing specification instrumentation.
        fn inspect(&self) -> Receipt
        {
            *self
        }
    }

    #[test]
    fn nested_trait_obligations_follow_the_selected_mode()
    {
        assert_eq!(Receipt::Preserved.inspect(), Receipt::Preserved);
        #[cfg(all(feature = "anodized", anodized_panic))]
        {
            let failure = std::panic::catch_unwind(|| Receipt::Altered.inspect())
                .expect_err("the inherited postcondition must reject the altered result");
            assert_postcondition_failure(failure);
        }
        #[cfg(not(all(feature = "anodized", anodized_panic)))]
        assert_eq!(Receipt::Altered.inspect(), Receipt::Altered);
    }

    #[cfg(feature = "anodized")]
    #[quenchant::spec(ensures: |ref output| output.is_ok())]
    /// Let the backend inspect an early error return without consuming its
    /// move-only payload.
    ///
    /// # Specification
    /// - ensures: enforcing instrumentation rejects an error return.
    /// - panics: the backend rejects the violated postcondition.
    ///
    /// # Errors
    /// Without enforcement, the input's I/O error is propagated unchanged.
    fn checked_exit(value: Result<Receipt, std::io::Error>) -> Result<Receipt, std::io::Error>
    {
        let value = value?;
        Ok(value)
    }

    #[cfg(all(feature = "anodized", anodized_panic))]
    /// Reject unrelated panics as evidence of specification enforcement.
    ///
    /// # Specification
    /// - ensures: requires the backend's postcondition failure diagnostic.
    /// - panics: an unrelated payload fails this test helper.
    fn assert_postcondition_failure(failure: Box<dyn core::any::Any + Send>)
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

    #[cfg(all(feature = "anodized", anodized_panic))]
    #[test]
    fn enforcing_postcondition_observes_early_error_return()
    {
        let failure =
            std::panic::catch_unwind(|| checked_exit(Err(std::io::Error::other("sentinel"))))
                .expect_err("the postcondition must reject the early error return");
        assert_postcondition_failure(failure);
        assert_eq!(
            checked_exit(Ok(Receipt::Preserved)).expect("valid result"),
            Receipt::Preserved
        );
    }

    #[cfg(all(feature = "anodized", not(anodized_panic)))]
    #[test]
    fn backend_selection_does_not_imply_enforcement()
    {
        let result = checked_exit(Err(std::io::Error::other("sentinel")));
        assert_eq!(
            result
                .expect_err("the body still returns its error")
                .to_string(),
            "sentinel"
        );
    }
}
