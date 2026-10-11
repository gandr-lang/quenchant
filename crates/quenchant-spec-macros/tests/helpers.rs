//! Exercise erasure and retention through the real host macro API.

#[cfg(test)]
mod tests
{
    #[spec_helper]
    use absent_dependency::Predicate;
    use quenchant_spec_macros::__retain_helper as retained;
    use quenchant_spec_macros::spec_helper;

    #[spec_helper]
    fn absent_helper(_: UnavailableType) -> UnavailableType
    {
        unavailable_body()
    }

    /// A retained helper's observable result.
    #[derive(Debug, Eq, PartialEq)]
    enum Marker
    {
        /// The retained helper survives expansion.
        Retained,
    }

    /// Return the retained marker.
    ///
    /// # Specification
    /// trivial.
    #[retained]
    const fn retained_helper() -> Marker
    {
        Marker::Retained
    }

    #[test]
    fn specification_only_items_follow_the_selected_mode()
    {
        #[spec_helper]
        const ERASED: UnavailableType = unavailable_initializer();
        #[retained]
        const PRESERVED: Marker = retained_helper();
        assert_eq!(PRESERVED, Marker::Retained);
    }
}
