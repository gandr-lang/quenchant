//! Nominal wrappers for the values this crate's own signatures carry.
//!
//! The gate crate holds itself to the project's `primitive_signature` gate: no
//! function defined here accepts or returns a bare primitive. Every predicate
//! answer, every count and every borrowed text fragment crosses a module
//! boundary inside one of the transparent wrappers defined below.

/// Define a transparent copyable semantic wrapper with bidirectional `From`
/// conversions.
macro_rules! semantic_copy {
    ($(#[$meta:meta])* struct $name:ident($inner:ty);) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub $inner);

        impl From<$inner> for $name {
            /// Wrap the inner value in the semantic type.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            /// Unwrap the semantic type to the value it carries.
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

/// Define a transparent borrowed-text semantic wrapper with `From` conversions.
macro_rules! semantic_borrowed_str {
    ($(#[$meta:meta])* struct $name:ident;) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name<'text>(pub &'text str);

        impl<'text> From<&'text str> for $name<'text> {
            /// Wrap the borrowed text in the semantic type.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: &'text str) -> Self {
                Self(value)
            }
        }

        impl<'text> From<&'text String> for $name<'text> {
            /// Wrap an owned string's text in the semantic type.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: &'text String) -> Self {
                Self(value.as_str())
            }
        }

        impl<'text> From<$name<'text>> for &'text str {
            /// Unwrap the semantic type to the text it borrows.
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
    /// A one-based source line number.
    struct LineNumber(usize);
);
semantic_copy!(
    /// Whether a rustdoc line opens a heading of any level.
    struct OpensHeading(bool);
);
semantic_copy!(
    /// Whether an `# Adequacy` block claims the required-trait-method
    /// declaration-only exemption.
    struct DeclarationOnly(bool);
);
semantic_copy!(
    /// Whether a listed test target is an integration-test target rather than a
    /// library or binary one.
    struct IntegrationTarget(bool);
);
semantic_copy!(
    /// Whether a workspace member pins its own toolchain and must therefore be
    /// listed from its own directory.
    struct PinsToolchain(bool);
);
semantic_copy!(
    /// Whether `cargo nextest` is available to list the workspace's tests.
    struct NextestAvailable(bool);
);
semantic_copy!(
    /// Whether a gate run produced at least one finding.
    struct GateFailed(bool);
);
semantic_copy!(
    /// A number of test aliases held by a catalog.
    struct AliasCount(usize);
);
semantic_copy!(
    /// Whether one target declared by Cargo holds the package's own
    /// documented source, or is a build script that only lives in the
    /// member's directory.
    struct HoldsPackageSource(bool);
);

semantic_borrowed_str!(
    /// Complete Rust source text of one file.
    struct SourceText;
);
semantic_borrowed_str!(
    /// One trimmed rustdoc line.
    struct DocLine;
);
semantic_borrowed_str!(
    /// A Cargo package name.
    struct PackageName;
);
semantic_borrowed_str!(
    /// A Cargo package identifier, as `cargo` spells it in machine output.
    struct PackageId;
);
semantic_borrowed_str!(
    /// An exact witness path as written in a `- witness:` bullet.
    struct WitnessPath;
);
semantic_borrowed_str!(
    /// A test alias as the test inventory reports it.
    struct TestAlias;
);
semantic_borrowed_str!(
    /// The label of one test target, `package` or `package::target`.
    struct TargetLabel;
);
semantic_borrowed_str!(
    /// The Cargo name of one test target.
    struct TargetName;
);
semantic_borrowed_str!(
    /// The Cargo kind of one target: `lib`, `bin`, `test`, `bench`.
    struct TargetKind;
);
semantic_borrowed_str!(
    /// The classification of one gate finding.
    struct FindingKind;
);
semantic_borrowed_str!(
    /// The prose detail of one gate finding.
    struct FindingDetail;
);
semantic_borrowed_str!(
    /// The command line a failed tool invocation was launched with.
    struct CommandLine;
);
semantic_borrowed_str!(
    /// A diagnostic message from a failed tool invocation or parse.
    struct ErrorMessage;
);
