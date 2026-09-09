//! Distinguish source addresses, package identities, witness names, and
//! verdicts.
//!
//! These transparent carriers keep primitive representations from becoming
//! interchangeable at gate interfaces. A representation-preserving conversion
//! does not validate a path or establish a finding; the producing stage
//! supplies that meaning and the evidence boundary it carries.

/// Copyable domain tags preserve representation through standard conversions.
macro_rules! semantic_copy {
    ($(#[$meta:meta])* struct $name:ident($inner:ty);) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub $inner);

        impl From<$inner> for $name {
            /// Tag the value without adding validation or changing its representation.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            /// Return the represented value through the standard conversion boundary.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

/// Borrowed domain text keeps its source lifetime across standard conversions.
macro_rules! semantic_borrowed_str {
    ($(#[$meta:meta])* struct $name:ident;) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name<'text>(pub &'text str);

        impl<'text> From<&'text str> for $name<'text> {
            /// Tag a text borrow without parsing or allocating.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: &'text str) -> Self {
                Self(value)
            }
        }

        impl<'text> From<&'text String> for $name<'text> {
            /// The string remains owned by its caller while its text gains a domain tag.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: &'text String) -> Self {
                Self(value.as_str())
            }
        }

        impl<'text> From<$name<'text>> for &'text str {
            /// Remove the domain tag while retaining the original text borrow.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: $name<'text>) -> Self {
                value.0
            }
        }
    };
}

semantic_copy!(
    /// Source coordinates count lines from one.
    struct LineNumber(usize);
);
semantic_copy!(
    /// Heading recognition marks the boundary of a documentation section.
    struct OpensHeading(bool);
);
semantic_copy!(
    /// An adequacy declaration claims the body-free required-method exemption.
    struct DeclarationOnly(bool);
);
semantic_copy!(
    /// Integration targets require target-prefixed witness aliases.
    struct IntegrationTarget(bool);
);
semantic_copy!(
    /// A member-local compiler selection requires listing from that member's
    /// directory.
    struct PinsToolchain(bool);
);
semantic_copy!(
    /// Availability determines whether nextest can supply an aggregate
    /// inventory.
    struct NextestAvailable(bool);
);
semantic_copy!(
    /// A nonempty finding set makes the policy run unsuccessful.
    struct GateFailed(bool);
);
semantic_copy!(
    /// Catalog cardinality counts package-scoped aliases.
    struct AliasCount(usize);
);
semantic_copy!(
    /// Source-target classification excludes package-level build-script roots.
    struct HoldsPackageSource(bool);
);

semantic_borrowed_str!(
    /// One file's complete text supplied to Rust parsing.
    struct SourceText;
);
semantic_borrowed_str!(
    /// One documentation line subject to the reader's whitespace rules.
    struct DocLine;
);
semantic_borrowed_str!(
    /// Package ownership name used by Cargo and witness resolution.
    struct PackageName;
);
semantic_borrowed_str!(
    /// Machine-output package identity before extracting its ownership name.
    struct PackageId;
);
semantic_borrowed_str!(
    /// Authored witness spelling used for exact lookup.
    struct WitnessPath;
);
semantic_borrowed_str!(
    /// Runnable name contributed by the selected inventory instrument.
    struct TestAlias;
);
semantic_borrowed_str!(
    /// Exposing-target identity spelled as a package or package/target pair.
    struct TargetLabel;
);
semantic_borrowed_str!(
    /// Target name supplied by Cargo for harness ownership.
    struct TargetName;
);
semantic_borrowed_str!(
    /// Cargo target category used to select witness-prefix behavior.
    struct TargetKind;
);
semantic_borrowed_str!(
    /// Stable diagnostic classification independent of repair prose.
    struct FindingKind;
);
semantic_borrowed_str!(
    /// Reader-directed repair information attached to a finding.
    struct FindingDetail;
);
semantic_borrowed_str!(
    /// Failed process identity with its selected invocation arguments.
    struct CommandLine;
);
semantic_borrowed_str!(
    /// Evidence text retained from an unsuccessful operation.
    struct ErrorMessage;
);
