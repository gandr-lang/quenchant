//! Nominal wrappers for the values this crate's own signatures carry.
//!
//! The gate crate holds itself to [`crate::PRIMITIVE_SIGNATURE`]: no function
//! defined here accepts or returns a bare primitive. Every predicate answer and
//! every borrowed rustdoc fragment crosses a module boundary inside one of the
//! transparent wrappers defined below.

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
    /// Whether a single-field struct already declares `#[repr(transparent)]`.
    struct TransparentReprDeclared(bool);
);
semantic_copy!(
    /// Whether a function is a method implementing a non-local trait.
    struct NonLocalTraitImpl(bool);
);
semantic_copy!(
    /// Whether a required `# Termination` bullet has a non-empty value.
    struct BulletHasValue(bool);
);
semantic_copy!(
    /// Whether the `- input recursion:` bullet claims no recursion over
    /// caller-supplied data.
    struct ClaimsNoInputRecursion(bool);
);
semantic_copy!(
    /// Whether a recursive SCC passes caller-input-derived data on some edge.
    struct HasInputDerivedRecursiveCall(bool);
);
semantic_copy!(
    /// Whether an expression references an input-derived local.
    struct ContainsDerivedBinding(bool);
);
semantic_copy!(
    /// Whether a provenance pass added a binding to the input-derived set.
    struct ProvenanceChanged(bool);
);
semantic_copy!(
    /// Whether a signature traversal emitted a primitive diagnostic.
    struct PrimitiveDiagnosticEmitted(bool);
);
semantic_copy!(
    /// Whether a primitive is one of the policy's banned signature primitives.
    struct DisallowedPrimitive(bool);
);
semantic_copy!(
    /// Whether an ADT is a semantic wrapper boundary for type-boundary linting.
    struct SemanticBoundaryAdt(bool);
);
semantic_copy!(
    /// Whether a semantic type contains a disallowed primitive before a
    /// nominal workspace boundary.
    struct ContainsPrimitive(bool);
);
semantic_copy!(
    /// Number of primitive-signature diagnostics a traversal has emitted.
    struct DiagnosticCount(usize);
);
semantic_copy!(
    /// A vertex position in a lint's adjacency table.
    struct Vertex(usize);
);
semantic_copy!(
    /// Whether an ADT forms no owning edge through any of its arguments.
    struct NonOwningAdt(bool);
);
semantic_copy!(
    /// Whether a generic parameter occupies an owned field position.
    struct OwnedParameter(bool);
);
semantic_copy!(
    /// Whether an owned-parameter pass set a flag it had not set before.
    struct OwnedParametersChanged(bool);
);
semantic_copy!(
    /// The position of a generic parameter in an item's generic arguments.
    struct ParameterIndex(usize);
);
semantic_copy!(
    /// Whether a rustdoc line opens a heading of any level.
    struct OpensHeading(bool);
);
semantic_copy!(
    /// Whether a recursion expectation is attached to the item it approves.
    struct ExpectationOnItem(bool);
);
semantic_copy!(
    /// Whether an `- hypothesis:` value names an adequacy ladder rung.
    struct NamesLadderRung(bool);
);
semantic_copy!(
    /// Whether a rustdoc line is an indented continuation of the line above.
    struct IndentedContinuation(bool);
);
semantic_copy!(
    /// Whether the documented item is a required method of a trait declaration.
    struct TraitRequiredMethod(bool);
);
semantic_copy!(
    /// Whether a `# Judgement` declaration names its item the checking
    /// judgement's direction.
    struct DirectionDeclared(bool);
);
semantic_copy!(
    /// Whether a match scrutinee's type is the declared checking direction.
    struct ScrutineeIsDirection(bool);
);
semantic_copy!(
    /// Whether a match scrutinee mentions a parameter declared to carry the
    /// expected type.
    struct ScrutineeMentionsExpected(bool);
);
semantic_copy!(
    /// Whether a binding is a parameter its function declares expected.
    struct DeclaredExpectedParameter(bool);
);
semantic_copy!(
    /// Whether an item's own name is syntax this crate's author wrote.
    struct AuthoredItem(bool);
);
semantic_copy!(
    /// Whether a `# Specification` section states the trivial marker beside
    /// another clause.
    struct MarkerBesideClause(bool);
);
semantic_copy!(
    /// Whether a `# Specification` section writes the trivial marker as a
    /// bullet.
    struct MarkerWrittenAsBullet(bool);
);
semantic_copy!(
    /// Whether the source text under an item's name span is the item's own
    /// identifier.
    struct NameSpanCarriesIdentifier(bool);
);
semantic_borrowed_str!(
    /// The text of one lint diagnostic.
    struct DiagnosticText;
);
semantic_borrowed_str!(
    /// The raw text of one rustdoc attribute, before line splitting.
    struct RustdocText;
);
semantic_borrowed_str!(
    /// A type's stable rustc def-path string.
    struct TypePath;
);
semantic_borrowed_str!(
    /// The source spelling of the field carrying one ownership edge.
    struct FieldLabel;
);
semantic_borrowed_str!(
    /// A trimmed rustdoc line of a `# Termination` section.
    struct TerminationLine;
);
semantic_borrowed_str!(
    /// A trimmed rustdoc line of any fixed-grammar section this crate reads.
    struct RustdocLine;
);
semantic_borrowed_str!(
    /// The value of an `- hypothesis:` bullet.
    struct HypothesisValue;
);
semantic_borrowed_str!(
    /// The literal prefix of a required `# Termination` bullet.
    struct BulletPrefix;
);
semantic_borrowed_str!(
    /// The heading opening one fixed-grammar rustdoc section.
    struct SectionHeading;
);
semantic_borrowed_str!(
    /// The name of one function parameter.
    struct ParameterName;
);
