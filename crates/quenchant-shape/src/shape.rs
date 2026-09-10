//! Domain boundaries preserve meaning that a payload representation cannot
//! state.
//!
//! Reason sites distinguish non-failure absence from an error channel. Nominal
//! declarations control representation exposure; delegated operators keep that
//! boundary while leaving the chosen operation's semantics with its owner.
//!
//! [`Maybe`] keeps absence outside implicit failure propagation. No default
//! reason is invented. Reason-site sealing and private transparent fields
//! constrain structure; their domain meanings remain authored obligations.

/// An absence site owns a closed reason vocabulary and its generic bound.
///
/// # Specification
/// - requires: the module names one absence site and every variant names a
///   concrete non-failure reason.
/// - ensures: only the declared enum implements that module's Reason trait.
/// - provides: caller-selected derives and documentation; no Error or Default
///   implementation is added.
/// - provides: sealing is a type-level guarantee; site meaning is the caller's
///   policy. Anodized predicates cannot quantify over implementations or
///   establish the semantic meaning of an enum variant.
/// - panics: none; this macro declares types only.
///
/// Each boundary selects its concrete reason enum. Generic code can use the
/// site's sealed trait without admitting an external reason implementation; the
/// container supplies no universal reason vocabulary.
///
/// ```rust
/// quenchant_shape::reason_enum! {
///     /// Why the cursor has no next value.
///     pub mod cursor {
///         /// Normal exhaustion of the cursor.
///         #[derive(Clone, Copy, Debug, Eq, PartialEq)]
///         pub enum Exhausted {
///             /// All values were consumed.
///             Exhausted,
///         }
///     }
/// }
/// let value: quenchant_shape::shape::Maybe<(), cursor::Exhausted> =
///     quenchant_shape::shape::Maybe::Absent(cursor::Exhausted::Exhausted);
/// assert!(matches!(value, quenchant_shape::shape::Maybe::Absent(_)));
/// ```
///
/// ```compile_fail
/// quenchant_shape::reason_enum! {
///     pub mod cursor {
///         pub enum Exhausted { Exhausted }
///     }
/// }
/// struct Unrelated;
/// impl cursor::Reason for Unrelated {}
/// ```
///
/// # Adequacy
/// - hypothesis: L0 the sealing compile-fail example above rejects an unrelated
///   implementer, exercised by the doc-test lane rather than named as a
///   witness, because a doctest is not a target the witness resolver
///   enumerates; L3 site-bound lookups preserve distinct reason variants.
/// - witness: `shape::tests::chaining_preserves_both_absence_transitions`
#[macro_export]
macro_rules! reason_enum {
    (
        $(#[$module_attribute:meta])*
        $module_visibility:vis mod $module:ident {
            $(#[$enum_attribute:meta])*
            $enum_visibility:vis enum $reason:ident { $($variants:tt)* }
        }
    ) => {
        $(#[$module_attribute])*
        $module_visibility mod $module {
            /// Prevent downstream code from extending this site's reason vocabulary.
            mod sealed {
                /// Membership evidence reserved for the declared enum.
                pub trait Sealed {}
            }

            /// The site's closed bound for generic absence-handling code.
            pub trait Reason: sealed::Sealed {}

            $(#[$enum_attribute])*
            $enum_visibility enum $reason { $($variants)* }

            impl sealed::Sealed for $reason {}
            impl Reason for $reason {}
        }
    };
}

/// Domain identity has transparent layout while construction remains locally
/// owned.
///
/// # Specification
/// - requires: the caller owns the wrapper and supplies meaningful
///   documentation and derives.
/// - ensures: the wrapper has one private field and transparent layout.
/// - provides: no constructor, Deref, From, Default, or operator implementation
///   beyond caller-selected derives.
/// - provides: field privacy and layout are declaration-level guarantees, not
///   value predicates. This macro generates no functions to annotate.
/// - panics: none; this macro declares a type only.
///
/// The owning module supplies invariant-preserving constructors and validation.
/// Standard trait implementations contain primitive access at the
/// representation border.
///
/// ```rust
/// quenchant_shape::nominal_type! {
///     /// Whether an action is permitted.
///     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
///     pub struct Permission(bool);
/// }
/// assert_eq!(
///     core::mem::size_of::<Permission>(),
///     core::mem::size_of::<bool>()
/// );
/// ```
///
/// ```compile_fail
/// mod domain {
///     quenchant_shape::nominal_type! { pub struct Permission(bool); }
/// }
/// let forged = domain::Permission(true);
/// ```
///
/// # Adequacy
/// - hypothesis: L0 the constructor compile-fail example above rejects external
///   primitive construction, exercised by the doc-test lane rather than named
///   as a witness, because a doctest is not a target the witness resolver
///   enumerates; L3 the wrapper preserves its representation's size and
///   alignment.
/// - witness: `shape::tests::nominal_wrapper_preserves_transparent_layout`
#[macro_export]
macro_rules! nominal_type {
    ($(#[$attribute:meta])* $visibility:vis struct $name:ident($representation:ty);) => {
        $(#[$attribute])*
        #[repr(transparent)]
        $visibility struct $name($representation);
    };
}

/// Operator syntax crosses a transparent representation only through its owning
/// trait implementation.
///
/// # Specification
/// - requires: invocation in the wrapper's owning module; a one-field tuple
///   wrapper; a safe operation path with the selected signature.
/// - ensures: the selected operation is called once with the inner operands,
///   and value results are rewrapped.
/// - provides: binary-by-value, unary-by-value, and mutable-assignment trait
///   forms; no implicit arithmetic policy.
/// - provides: generated methods carry `#[spec]` without predicates. The safe
///   call signature is type-checked; ownership and operation meaning are caller
///   obligations. Replaying an arbitrary operation could duplicate effects, and
///   move-only operands offer no independent equality observation.
/// - panics: exactly when the chosen operation panics.
/// - intension: adds no allocation, cloning, or unsafe block.
///
/// A safe arithmetic operator requires a permanently strict delegate. Unsafe
/// fast entry points do not satisfy that boundary. Assignment delegates borrow
/// the left representation mutably.
///
/// ```rust
/// quenchant_shape::nominal_type! {
///     /// Permission combined by logical union.
///     #[derive(Clone, Copy, Debug, Eq, PartialEq)]
///     struct Permission(bool);
/// }
/// quenchant_shape::delegate_ops!(binary Permission, Add::add => core::ops::BitOr::bitor);
/// assert_eq!(Permission(false) + Permission(true), Permission(true));
/// ```
///
/// # Adequacy
/// - hypothesis: L3 distinct permission inputs distinguish selected binary,
///   unary, and assignment semantics from ignored operands or unchanged state.
/// - witness: `shape::tests::selected_operators_preserve_the_permission_domain`
#[macro_export]
macro_rules! delegate_ops {
    (binary $wrapper:ty, $trait:ident:: $method:ident => $operation:path) => {
        impl ::core::ops::$trait for $wrapper
        {
            type Output = Self;

            /// The binary delegate determines meaning within the wrapper's nominal
            /// boundary.
            ///
            /// # Specification
            /// - requires: the selected operation is a safe two-argument path over the
            ///   wrapper's representation, permanently strict where it is arithmetic.
            /// - ensures: calls that operation once on the two inner values and rewraps
            ///   its result.
            /// - provides: the wrapper's own operator, with no arithmetic policy of the
            ///   container's choosing.
            /// - panics: exactly when the selected operation panics.
            /// - intension: adds no allocation, cloning, or unsafe block.
            #[inline]
            #[quenchant::spec]
            fn $method(
                self,
                rhs: Self,
            ) -> Self
            {
                Self($operation(self.0, rhs.0))
            }
        }
    };
    (unary $wrapper:ty, $trait:ident:: $method:ident => $operation:path) => {
        impl ::core::ops::$trait for $wrapper
        {
            type Output = Self;

            /// The unary delegate determines meaning within the wrapper's nominal
            /// boundary.
            ///
            /// # Specification
            /// - requires: the selected operation is a safe one-argument path over the
            ///   wrapper's representation.
            /// - ensures: calls that operation once on the inner value and rewraps its
            ///   result.
            /// - provides: the wrapper's own unary operator.
            /// - panics: exactly when the selected operation panics.
            /// - intension: adds no allocation, cloning, or unsafe block.
            #[inline]
            #[quenchant::spec]
            fn $method(self) -> Self
            {
                Self($operation(self.0))
            }
        }
    };
    (assign $wrapper:ty, $trait:ident:: $method:ident => $operation:path) => {
        impl ::core::ops::$trait for $wrapper
        {
            /// Assignment lends the representation to the selected in-place operation.
            ///
            /// # Specification
            /// - requires: the selected operation is a safe path taking the
            ///   representation by mutable reference and the operand by value.
            /// - ensures: calls that operation once on the borrowed left representation
            ///   and the right inner value, and returns nothing.
            /// - provides: the wrapper's own assignment operator.
            /// - panics: exactly when the selected operation panics.
            /// - intension: adds no allocation, cloning, or unsafe block.
            #[inline]
            #[quenchant::spec]
            fn $method(
                &mut self,
                rhs: Self,
            )
            {
                $operation(&mut self.0, rhs.0);
            }
        }
    };
}

reason_enum! {
    /// Querying absence introduces a separate site from the original value's absence.
    pub mod absence_query {
        /// Evidence that a reason query encountered a present payload.
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum ValuePresent {
            /// A value occupies the original container, so no absence reason exists there.
            Present,
        }
    }
}

/// Preserve either an owned value or this site's non-failure absence evidence.
///
/// # Specification
/// - requires: the caller's reason vocabulary denotes one closed absence site,
///   not an erased or catch-all failure channel.
/// - ensures: absence retains its exact reason as data.
/// - provides: no implicit failure propagation and no invented default reason.
/// - provides: the type preserves its payload but cannot prove the domain
///   meaning of an arbitrary caller-supplied `Reason`; that obligation remains
///   explicit rather than replaced by a weaker value predicate.
///
/// ```compile_fail
/// use quenchant_shape::shape::Maybe;
/// enum Exhausted { Exhausted }
/// fn cannot_propagate(value: Maybe<(), Exhausted>) -> Result<(), core::convert::Infallible> {
///     value?;
///     Ok(())
/// }
/// ```
///
/// # Adequacy
/// - hypothesis: L0 the propagation compile-fail example above rejects use of
///   the error channel, exercised by the doc-test lane rather than named as a
///   witness, because a doctest is not a target the witness resolver
///   enumerates; L3 both arms preserve move-only data and distinguish absence
///   from present values.
/// - witness: `shape::tests::mapping_preserves_absence_and_moves_values`
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
#[cfg_attr(quenchant_compiler_policy, rustc_diagnostic_item = "quenchant_maybe")]
pub enum Maybe<Value, Reason>
{
    /// An available payload; no absence evidence is manufactured alongside it.
    Present(Value),
    /// An unavailable value with an explicit reason that is not itself failure.
    Absent(Reason),
}

impl<Value, Reason> Maybe<Value, Reason>
{
    /// Transform availability without replaying a move-only mapper.
    ///
    /// # Specification
    /// - ensures: output and input agree on whether a value is present.
    /// - ensures: a present value reaches the mapper exactly once; an absence
    ///   bypasses it and retains the original reason.
    /// - provides: the executable predicate observes only the variant relation.
    ///   These generic bounds expose no payload equality or callback history;
    ///   replaying `FnOnce` would change the computation being checked.
    /// - panics: propagates a panic from the invoked mapper.
    /// - intension: the ordinary wrapper adds no allocation or `Clone` bound;
    ///   callback work and selected instrumentation are separate costs.
    ///
    /// # Adequacy
    /// - hypothesis: L3 present/absent transition witnesses distinguish
    ///   callback execution and reason preservation.
    /// - witness: `shape::tests::mapping_preserves_absence_and_moves_values`
    #[inline]
    #[quenchant::spec(
        captures: was_present = matches!(self, Self::Present(_)),
        ensures: |ref output| matches!(output, Maybe::Present(_)) == was_present,
    )]
    pub fn map<Mapped, Mapper>(
        self,
        mapper: Mapper,
    ) -> Maybe<Mapped, Reason>
    where
        Mapper: FnOnce(Value) -> Mapped,
    {
        match self {
            | Self::Present(value) => Maybe::Present(mapper(value)),
            | Self::Absent(reason) => Maybe::Absent(reason),
        }
    }

    /// Compose within the same reason domain while preserving an existing
    /// absence.
    ///
    /// # Specification
    /// - ensures: an absent input remains absent with the same reason and never
    ///   invokes the continuation.
    /// - ensures: a present input invokes the continuation once and retains
    ///   whichever value or reason it produces.
    /// - provides: the predicate observes preservation of existing absence.
    ///   Exact payload and callback-history claims remain obligations even
    ///   though this generic interface exposes no equality or replay oracle.
    /// - panics: propagates a panic from the invoked continuation.
    /// - intension: the ordinary wrapper moves data without allocating or
    ///   requiring `Clone`; callback and instrumentation costs remain separate.
    ///
    /// # Adequacy
    /// - hypothesis: L3 chaining witnesses distinguish existing absence,
    ///   produced absence, and produced value.
    /// - witness: `shape::tests::chaining_preserves_both_absence_transitions`
    #[inline]
    #[quenchant::spec(
        captures: was_present = matches!(self, Self::Present(_)),
        ensures: |ref output| was_present || matches!(output, Maybe::Absent(_)),
    )]
    pub fn and_then<Mapped, Continuation>(
        self,
        continuation: Continuation,
    ) -> Maybe<Mapped, Reason>
    where
        Continuation: FnOnce(Value) -> Maybe<Mapped, Reason>,
    {
        match self {
            | Self::Present(value) => continuation(value),
            | Self::Absent(reason) => Maybe::Absent(reason),
        }
    }

    /// Borrow absence evidence without treating an available value as a missing
    /// answer.
    ///
    /// # Specification
    /// - ensures: an absent input exposes its borrowed reason; a present input
    ///   returns the query site's `ValuePresent::Present` reason instead.
    /// - provides: a const query without executable instrumentation. The
    ///   selected backend's runtime closure machinery is not a const
    ///   interpretation, so the authored statement preserves the const API.
    /// - panics: none.
    /// - intension: the query neither clones nor allocates.
    ///
    /// # Adequacy
    /// - hypothesis: L3 both variants distinguish a borrowed reason from a
    ///   present value; move-only input remains usable.
    /// - witness: `shape::tests::reason_query_preserves_the_original_value`
    #[inline]
    pub const fn absent_reason(&self) -> Maybe<&Reason, absence_query::ValuePresent>
    {
        match *self {
            | Self::Present(_) => Maybe::Absent(absence_query::ValuePresent::Present),
            | Self::Absent(ref reason) => Maybe::Present(reason),
        }
    }

    /// Make a caller-chosen promotion from absence evidence to a failure
    /// channel.
    ///
    /// # Specification
    /// - requires: this boundary treats absence as failure and the mapper names
    ///   that failure without erasing the site's meaning.
    /// - ensures: success corresponds exactly to a present input; that path
    ///   retains the value and does not invoke the mapper.
    /// - ensures: an absence invokes the mapper once and retains its error.
    /// - provides: the predicate observes success versus failure, not the
    ///   adequacy of the caller's policy or exact move-only payload equality.
    /// - fails: reports only the concrete error returned by the mapper.
    /// - panics: propagates a panic from the invoked mapper.
    /// - intension: the ordinary wrapper adds no allocation; mapper and
    ///   instrumentation work are outside that bound.
    ///
    /// # Errors
    /// The error is the mapper's result for the original absence reason.
    /// `core::error::Error` is required at this explicit boundary; `Maybe`
    /// itself does not acquire an implicit error interpretation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 only the absent arm invokes the failure mapper;
    ///   distinct reasons reach distinct errors.
    /// - witness: `shape::tests::failure_promotion_is_explicit_and_reason_sensitive`
    #[inline]
    #[quenchant::spec(
        captures: was_present = matches!(self, Self::Present(_)),
        ensures: |ref output| output.is_ok() == was_present,
    )]
    pub fn into_result<Failure, Mapper>(
        self,
        mapper: Mapper,
    ) -> Result<Value, Failure>
    where
        Failure: core::error::Error,
        Mapper: FnOnce(Reason) -> Failure,
    {
        match self {
            | Self::Present(value) => Ok(value),
            | Self::Absent(reason) => Err(mapper(reason)),
        }
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
