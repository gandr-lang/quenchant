//! Non-failure absence preserves its concrete reason until a handler decides
//! otherwise.
//!
//! [`Maybe`] is a distinct enum. It has no Try implementation, default reason,
//! or implicit conversion to Option or Result. The exported scaffolds produce
//! closed reason sites and private-field transparent types, without allocating.

/// Declare one reason enum and a sealed reason trait scoped to its site.
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
/// The container does not impose a shared catch-all Reason trait. A caller's
/// boundary names the concrete enum, or bounds its local generic helper by the
/// site's sealed trait. No external implementation can widen that site.
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
            /// Seal the reason set to the enum declared at this site.
            mod sealed {
                /// Marker implemented only by this site's reason enum.
                pub trait Sealed {}
            }

            /// A reason belonging to this closed absence site.
            pub trait Reason: sealed::Sealed {}

            $(#[$enum_attribute])*
            $enum_visibility enum $reason { $($variants)* }

            impl sealed::Sealed for $reason {}
            impl Reason for $reason {}
        }
    };
}

/// Declare a concrete transparent newtype with a private representation.
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
/// Constructors and validation stay with the caller. A primitive is unpacked
/// inside a standard trait implementation rather than a new public accessor.
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

/// Delegate a chosen operator at a transparent wrapper's standard-trait border.
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
/// For arithmetic, choose a permanently strict function when implementing a
/// safe operator trait; an unsafe fast entry point cannot be called here.
/// Assignment operations receive the left inner value by mutable reference.
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

            /// Combine two wrappers through the selected binary operation.
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

            /// Transform one wrapper through the selected unary operation.
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
            /// Update this wrapper in place through the selected operation.
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
    /// The absence-reason query has its own closed reason site.
    pub mod absence_query {
        /// Why a query cannot return an absence reason.
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub enum ValuePresent {
            /// The original Maybe contains a value, not an absence reason.
            Present,
        }
    }
}

/// A value or a concrete, non-failure absence reason.
///
/// # Specification
/// - requires: Reason is a closed enum naming this absence site, never an
///   erased or catch-all error type.
/// - ensures: the absent arm retains that reason as data.
/// - provides: no implicit conversion to a weaker channel and no default
///   reason.
/// - provides: the enum's payload types retain the reason; a value predicate
///   cannot establish that a caller-defined type denotes one closed absence
///   site. No catch-all bound or weaker predicate replaces that obligation.
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
pub enum Maybe<Value, Reason>
{
    /// The value exists.
    Present(Value),
    /// The value does not exist for this concrete non-failure reason.
    Absent(Reason),
}

impl<Value, Reason> Maybe<Value, Reason>
{
    /// Transform a present value without changing absence.
    ///
    /// # Specification
    /// - ensures: the output is Present exactly when the input is Present.
    /// - ensures: invokes the mapper once on Present; preserves the exact
    ///   reason on Absent without invoking it.
    /// - provides: the predicate checks variant preservation. Exact move-only
    ///   payload equality and `FnOnce` call history have no generic
    ///   observation; replaying the mapper would consume it twice or duplicate
    ///   its effects.
    /// - panics: only if the invoked mapper panics.
    /// - intension: moves the value or reason; adds no allocation or Clone
    ///   bound.
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

    /// Chain another computation in the same concrete absence domain.
    ///
    /// # Specification
    /// - ensures: an absent input produces an absent output.
    /// - ensures: invokes the continuation once on Present and returns its
    ///   value or reason unchanged; preserves an existing absence without
    ///   invoking it.
    /// - provides: the predicate checks the existing-absence transition. Exact
    ///   move-only payload equality and `FnOnce` call history have no generic
    ///   observation; replaying the continuation would change its semantics.
    /// - panics: only if the invoked continuation panics.
    /// - intension: moves data and adds no allocation or Clone bound.
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

    /// Borrow an absence reason without erasing why no reason is returned.
    ///
    /// # Specification
    /// - ensures: an absent input returns its borrowed reason; a present input
    ///   returns the query site's `ValuePresent` reason.
    /// - provides: a const query without `#[spec]`. The pinned Anodized macro
    ///   calls non-const `eval_once` even for an empty specification, producing
    ///   E0015 in a const function; the const API is preserved.
    /// - panics: none.
    /// - intension: borrows without cloning or allocating.
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

    /// Promote absence to failure at an explicit boundary handler.
    ///
    /// # Specification
    /// - requires: this boundary treats absence as a real failure and the
    ///   mapper names that failure.
    /// - ensures: the output is Ok exactly when the input is Present.
    /// - ensures: Present becomes Ok without invoking the mapper; Absent
    ///   invokes it once and returns Err.
    /// - provides: the predicate checks success versus failure. The boundary's
    ///   failure policy is semantic; exact move-only payload equality and
    ///   `FnOnce` call history cannot be checked without changing this API.
    /// - fails: only the concrete error supplied by the mapper.
    /// - panics: only if the invoked mapper panics.
    /// - intension: moves data and adds no allocation.
    ///
    /// # Errors
    /// Returns the mapper's concrete Error for the absent arm. The error type
    /// must implement `core::error::Error`; absence itself does not implement
    /// it.
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
