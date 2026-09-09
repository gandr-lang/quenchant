//! Distinct carriers for analysis answers, source fragments, and graph
//! positions.
//!
//! The wrappers prevent those domains from crossing signatures as
//! interchangeable primitive values. Their constructors preserve
//! representation, not semantic validity: each producing analysis still owes
//! the meaning its result claims. Standard-trait conversions are the explicit
//! representation boundary.

/// Copyable analysis results retain domain identity through standard
/// conversions.
macro_rules! semantic_copy {
    ($(#[$meta:meta])* struct $name:ident($inner:ty);) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(pub $inner);

        impl From<$inner> for $name {
            /// Representation is preserved while the value acquires its analysis-domain tag.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            /// Standard conversion removes the tag without changing the represented answer.
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

/// Borrowed analysis text retains its source lifetime without allocating.
macro_rules! semantic_borrowed_str {
    ($(#[$meta:meta])* struct $name:ident;) => {
        $(#[$meta])*
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name<'text>(pub &'text str);

        impl<'text> From<&'text str> for $name<'text> {
            /// Text acquires a domain tag without parsing or validation.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: &'text str) -> Self {
                Self(value)
            }
        }

        impl<'text> From<&'text String> for $name<'text> {
            /// The caller keeps string ownership while lending its text to this domain.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: &'text String) -> Self {
                Self(value.as_str())
            }
        }

        impl<'text> From<$name<'text>> for &'text str {
            /// The original text borrow survives removal of its domain tag.
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
    /// Explicit transparent layout discharges the single-field representation
    /// requirement.
    struct TransparentReprDeclared(bool);
);
semantic_copy!(
    /// Foreign trait identity determines whether its required signature owns
    /// the boundary.
    struct NonLocalTraitImpl(bool);
);
semantic_copy!(
    /// A termination field supplies text beyond its required prefix.
    struct BulletHasValue(bool);
);
semantic_copy!(
    /// The author denies recursion over caller-provided data.
    struct ClaimsNoInputRecursion(bool);
);
semantic_copy!(
    /// A cycle edge carries data traced back to its caller's parameters.
    struct HasInputDerivedRecursiveCall(bool);
);
semantic_copy!(
    /// Referencing a derived binding makes the inspected expression
    /// input-dependent.
    struct ContainsDerivedBinding(bool);
);
semantic_copy!(
    /// Newly derived bindings require another provenance pass.
    struct ProvenanceChanged(bool);
);
semantic_copy!(
    /// A reported descendant can discharge the pending path-level diagnostic.
    struct PrimitiveDiagnosticEmitted(bool);
);
semantic_copy!(
    /// The selected primitive requires a nominal signature boundary.
    struct DisallowedPrimitive(bool);
);
semantic_copy!(
    /// A local transparent ADT terminates boundary traversal.
    struct SemanticBoundaryAdt(bool);
);
semantic_copy!(
    /// Primitive exposure remains reachable before an admitted nominal
    /// boundary.
    struct ContainsPrimitive(bool);
);
semantic_copy!(
    /// Diagnostic snapshots distinguish a silent subtree from one that already
    /// reported.
    struct DiagnosticCount(usize);
);
semantic_copy!(
    /// Adjacency-table identity, distinct from a compiler definition
    /// identifier.
    struct Vertex(usize);
);
semantic_copy!(
    /// This ADT contributes no ownership through its generic arguments.
    struct NonOwningAdt(bool);
);
semantic_copy!(
    /// Generic substitution at this position can participate in an owning
    /// field.
    struct OwnedParameter(bool);
);
semantic_copy!(
    /// New owned-parameter evidence requires another fixed-point pass.
    struct OwnedParametersChanged(bool);
);
semantic_copy!(
    /// Generic-argument position used when ownership transfers through
    /// substitution.
    struct ParameterIndex(usize);
);
semantic_copy!(
    /// Heading recognition determines a documentation section's end.
    struct OpensHeading(bool);
);
semantic_copy!(
    /// Exception approval belongs to this item rather than an enclosing scope.
    struct ExpectationOnItem(bool);
);
semantic_copy!(
    /// An isolated ladder token occurs in the authored hypothesis.
    struct NamesLadderRung(bool);
);
semantic_copy!(
    /// Indentation associates this text with the preceding bullet.
    struct IndentedContinuation(bool);
);
semantic_copy!(
    /// A required trait method has no implementation body at this declaration
    /// site.
    struct TraitRequiredMethod(bool);
);
semantic_copy!(
    /// Authored metadata assigns the direction role to its type declaration.
    struct DirectionDeclared(bool);
);
semantic_copy!(
    /// Type inspection recognizes the scrutinee as the declared direction.
    struct ScrutineeIsDirection(bool);
);
semantic_copy!(
    /// Parameter tracking recognizes expected-type data within the scrutinee.
    struct ScrutineeMentionsExpected(bool);
);
semantic_copy!(
    /// Binding identity matches an expected parameter declared by its function.
    struct DeclaredExpectedParameter(bool);
);
semantic_copy!(
    /// Source provenance attributes the item's name syntax to this crate.
    struct AuthoredItem(bool);
);
semantic_copy!(
    /// A trivial marker conflicts with another clause in the same section.
    struct MarkerBesideClause(bool);
);
semantic_copy!(
    /// Bullet syntax turns the required bare trivial marker into a malformed
    /// declaration.
    struct MarkerWrittenAsBullet(bool);
);
semantic_copy!(
    /// Name-span text matches the item's identifier rather than its producing
    /// macro.
    struct NameSpanCarriesIdentifier(bool);
);
semantic_borrowed_str!(
    /// Diagnostic wording selected by a compiler-policy decision.
    struct DiagnosticText;
);
semantic_borrowed_str!(
    /// Attribute text before reader-specific line interpretation.
    struct RustdocText;
);
semantic_borrowed_str!(
    /// Stable type identity used when ordering or explaining ownership.
    struct TypePath;
);
semantic_borrowed_str!(
    /// Authored field spelling attached to an ownership edge.
    struct FieldLabel;
);
semantic_borrowed_str!(
    /// Termination grammar input after surrounding whitespace is removed.
    struct TerminationLine;
);
semantic_borrowed_str!(
    /// One source documentation line undergoing section-specific
    /// interpretation.
    struct RustdocLine;
);
semantic_borrowed_str!(
    /// Hypothesis text inspected for an evidence-ladder token.
    struct HypothesisValue;
);
semantic_borrowed_str!(
    /// Exact field label required at one termination-grammar position.
    struct BulletPrefix;
);
semantic_borrowed_str!(
    /// Exact section label used by the shared documentation reader.
    struct SectionHeading;
);
semantic_borrowed_str!(
    /// Authored parameter spelling matched against compiler binding identity.
    struct ParameterName;
);
