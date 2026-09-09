//! Arithmetic meaning is selected explicitly, not by optimization profile.
//!
//! The sealed operation trait admits the standard machine-integer
//! representations. Strict operations and standard operators retain their safe
//! panic behavior; checked operations retain a typed cause; wrapping and
//! saturation select different value relations.
//!
//! Only the unprefixed `fast` family is unchecked. Its safety precondition
//! still belongs to the caller when instrumentation is enabled: optional
//! checking is not permission to invoke an unsafe operation on invalid inputs.
//!
//! The nominal representation adds no allocation. Predicate instrumentation and
//! its failure reporting are a separate cost and evidence boundary.

/// A private transparent representation boundary; arithmetic uses the sealed
/// integer trait.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
#[must_use]
pub struct Int<Representation>(Representation);

/// The attempted relation retained in a typed arithmetic failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Operation
{
    /// Width-bounded addition without a separate carry result.
    Add,
    /// Subtraction in the operands' signed or unsigned representation.
    Sub,
    /// Multiplication at the operands' representation width.
    Mul,
    /// Integer division truncated toward zero, not floor division.
    Div,
    /// Remainder paired with truncating integer division.
    Rem,
}

/// Invalid arithmetic inputs remain distinguishable without parsing a panic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArithmeticError
{
    /// Includes signed `MIN / -1` and `MIN % -1`, as well as ordinary range
    /// overflow.
    Overflow(Operation),
    /// Separates a zero right operand from the signed overflow pair.
    ZeroDivisor(Operation),
}

impl core::fmt::Display for Operation
{
    /// Operation labels used by arithmetic diagnostics.
    ///
    /// # Specification
    /// - provides: one of `addition`, `subtraction`, `multiplication`,
    ///   `division`, `remainder`, one per variant and distinct from every
    ///   other.
    /// - fails: returns the receiving formatter's write failure unchanged.
    /// - panics: none.
    ///
    /// # Errors
    /// - [`core::fmt::Error`]: the receiving formatter refused the write.
    #[inline]
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result
    {
        f.write_str(match *self {
            | Self::Add => "addition",
            | Self::Sub => "subtraction",
            | Self::Mul => "multiplication",
            | Self::Div => "division",
            | Self::Rem => "remainder",
        })
    }
}

impl core::fmt::Display for ArithmeticError
{
    /// Diagnostics retain both the attempted operation and its failure class.
    ///
    /// # Specification
    /// - provides: the operation's name, then `: result is out of range` for
    ///   [`ArithmeticError::Overflow`] and `: right operand is zero` for
    ///   [`ArithmeticError::ZeroDivisor`], so operation and cause are both
    ///   recoverable from the rendered text.
    /// - fails: returns the receiving formatter's write failure unchanged.
    /// - panics: none.
    ///
    /// # Errors
    /// - [`core::fmt::Error`]: the receiving formatter refused the write.
    #[inline]
    fn fmt(
        &self,
        f: &mut core::fmt::Formatter<'_>,
    ) -> core::fmt::Result
    {
        match *self {
            | Self::Overflow(operation) => write!(f, "{operation}: result is out of range"),
            | Self::ZeroDivisor(operation) => write!(f, "{operation}: right operand is zero"),
        }
    }
}

impl core::error::Error for ArithmeticError
{
}

/// Prevent external implementations from changing the arithmetic
/// specifications.
mod sealed
{
    /// Membership is restricted to the nominal representations implemented
    /// here.
    pub trait Sealed
    {
    }
}

/// Machine arithmetic whose representation and operation policy are explicit.
///
/// # Specification
/// - ensures: operations have the primitive representation's exact mathematical
///   semantics.
/// - provides: all twelve machine-integer representations through [`Int`].
///   Anodized's trait attribute enables method specifications; it accepts no
///   trait-level predicates. Representation-specific predicates live on each
///   implementation.
/// - panics: strict operations reject overflow and zero divisors in every
///   profile.
/// - intension: operations allocate nothing; constant operation selectors
///   inline away.
///
/// # Adequacy
/// - hypothesis: L2 differential agreement with the primitive specification on
///   every width's five-point `MIN`/`MAX` grid distinguishes wrong operations
///   and arithmetic families; exact overflow versus zero-divisor variants and
///   strict operator panics are L3 pointwise residues.
/// - witness: `arith::tests::u128_boundaries`
/// - witness: `arith::tests::i8_negative_boundaries`
#[cfg_attr(
    feature = "anodized",
    expect(
        non_upper_case_globals,
        reason = "The published backend emits lowercase associated qualifier constants."
    )
)]
#[cfg_attr(
    feature = "anodized",
    expect(
        clippy::missing_inline_in_public_items,
        reason = "The published backend generates public default trait helpers without inline attributes."
    )
)]
#[quenchant::spec]
pub trait Integer: sealed::Sealed + Copy + Default + Eq + core::fmt::Debug
{
    /// Exact arithmetic with profile-independent panic boundaries.
    ///
    /// # Specification
    /// - ensures: returns the exact representable result, truncating division
    ///   toward zero.
    /// - panics: overflow, or a zero divisor for division or remainder.
    ///
    /// # Adequacy
    /// - hypothesis: L2 the valid boundary-grid pairs return the primitive
    ///   specification's exact result; `MAX + 1`, `MIN - 1`, `MAX * 2`, zero
    ///   divisors, and signed `MIN / -1` or `MIN % -1` witness the strict panic
    ///   L3 pointwise residue in both profiles.
    /// - witness: `arith::tests::u128_boundaries`
    /// - witness: `arith::tests::i8_negative_boundaries`
    #[must_use]
    #[spec(ensures: |output| self.checked(rhs, operation) == Ok(output))]
    fn strict(
        self,
        rhs: Self,
        operation: Operation,
    ) -> Self;

    /// Exact arithmetic whose invalid inputs remain distinguishable errors.
    ///
    /// # Specification
    /// - ensures: returns the exact representable result.
    /// - provides: exact-result predicates on each implementation; the generic
    ///   trait exposes no primitive representation for an independent
    ///   predicate.
    /// - fails: returns [`ArithmeticError::Overflow`] or
    ///   [`ArithmeticError::ZeroDivisor`] naming the operation.
    /// - panics: none.
    ///
    /// # Errors
    /// Returns overflow when the primitive checked operation has no result; a
    /// zero right operand in division or remainder is classified
    /// separately.
    ///
    /// # Adequacy
    /// - hypothesis: L2 every boundary-grid result agrees with the primitive
    ///   checked specification. L3 residues: zero divisors produce the exact
    ///   `ZeroDivisor` operation, while signed `MIN / -1` and `MIN % -1` retain
    ///   the distinct `Overflow` operation.
    /// - witness: `arith::tests::u128_boundaries`
    /// - witness: `arith::tests::isize_negative_boundaries`
    #[spec()]
    fn checked(
        self,
        rhs: Self,
        operation: Operation,
    ) -> Result<Self, ArithmeticError>;

    /// Fixed-width arithmetic interpreted in the modular domain.
    ///
    /// # Specification
    /// - ensures: overflow wraps modulo the representation width; signed MIN /
    ///   -1 wraps to MIN.
    /// - provides: modular predicates on each implementation; the generic trait
    ///   exposes neither representation width nor modular primitives.
    /// - panics: a zero divisor for division or remainder.
    ///
    /// # Adequacy
    /// - hypothesis: L2 differential agreement with primitive modular
    ///   arithmetic over the boundary grid distinguishes wrapping from strict
    ///   and clamped results. L3 residues: signed `MIN / -1` returns `MIN`,
    ///   `MIN % -1` returns zero, and zero divisors still panic.
    /// - witness: `arith::tests::u128_boundaries`
    /// - witness: `arith::tests::i8_negative_boundaries`
    #[must_use]
    #[spec()]
    fn wrapping(
        self,
        rhs: Self,
        operation: Operation,
    ) -> Self;

    /// Arithmetic interpreted by projection onto the representable interval.
    ///
    /// # Specification
    /// - ensures: overflow clamps; remainder is exact because its mathematical
    ///   result fits.
    /// - provides: clamping predicates on each implementation; the generic
    ///   trait exposes neither representation bounds nor clamping primitives.
    /// - panics: a zero divisor for division or remainder.
    ///
    /// # Adequacy
    /// - hypothesis: L2 differential agreement with the primitive clamp
    ///   specification over the boundary grid distinguishes clamping from
    ///   wrapping. L3 residues: signed `MIN / -1` clamps to `MAX`, remainder
    ///   stays exact including zero for `MIN % -1`, and zero divisors panic.
    /// - witness: `arith::tests::u128_boundaries`
    /// - witness: `arith::tests::i8_negative_boundaries`
    #[must_use]
    #[spec()]
    fn saturating(
        self,
        rhs: Self,
        operation: Operation,
    ) -> Self;

    /// Exact arithmetic admitted by a caller-held validity proof.
    ///
    /// # Specification
    /// - requires: the operation has a representable result and no zero
    ///   divisor.
    /// - ensures: returns the same result as strict arithmetic on every valid
    ///   input.
    /// - panics: none.
    ///
    /// # Safety
    /// The corresponding [`Integer::checked`] call must return `Ok`. In
    /// particular, signed MIN / -1 and MIN % -1 violate the precondition,
    /// even though the mathematical remainder is zero.
    ///
    /// # Adequacy
    /// - hypothesis: L2 under `fast`, each boundary-grid pair admitted by the
    ///   primitive checked specification returns that exact value; the
    ///   independent specification, not the implementation under test,
    ///   establishes the unchecked precondition.
    /// - witness: `arith::tests::u128_boundaries`
    /// - witness: `arith::tests::i128_boundaries`
    #[must_use]
    #[spec(
        requires: self.checked(rhs, operation).is_ok(),
        ensures: |output| self.checked(rhs, operation) == Ok(output),
    )]
    unsafe fn unchecked(
        self,
        rhs: Self,
        operation: Operation,
    ) -> Self;
}

/// Keep primitive conversion and arithmetic within each representation's trait
/// boundary.
macro_rules! integers {
    ($($representation:ty),+ $(,)?) => {$ (
        impl sealed::Sealed for Int<$representation> {}

        impl Int<$representation> {
            /// Lower endpoint of this nominal representation's integer interval.
            pub const MIN: Self = Self(<$representation>::MIN);
            /// Upper endpoint of this nominal representation's integer interval.
            pub const MAX: Self = Self(<$representation>::MAX);
        }

        // Conversion traits own primitive ingress and egress.
        impl From<$representation> for Int<$representation> {
            /// Primitive ingress preserves the complete integer value.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: $representation) -> Self { Self(value) }
        }
        impl From<Int<$representation>> for $representation {
            /// Primitive egress preserves the value while removing its nominal boundary.
            ///
            /// # Specification
            /// trivial.
            #[inline]
            fn from(value: Int<$representation>) -> Self { value.0 }
        }

        #[quenchant::spec]
        impl Integer for Int<$representation> {
            /// Representation-specific strictness remains independent of build profile.
            ///
            /// # Specification
            /// - provides: the primitive `strict_add`, `strict_sub`,
            ///   `strict_mul`, `strict_div`, or `strict_rem` result for the
            ///   named operation, renamed back into [`Int`].
            /// - panics: overflow, or a zero divisor for division or remainder,
            ///   in every profile.
            #[inline]
            fn strict(self, rhs: Self, operation: Operation) -> Self {
                Self(match operation {
                    Operation::Add => self.0.strict_add(rhs.0),
                    Operation::Sub => self.0.strict_sub(rhs.0),
                    Operation::Mul => self.0.strict_mul(rhs.0),
                    Operation::Div => self.0.strict_div(rhs.0),
                    Operation::Rem => self.0.strict_rem(rhs.0),
                })
            }

            /// Representation failures retain the operation that could not produce a value.
            ///
            /// # Specification
            /// - ensures: the result agrees with the primitive `checked_*`
            ///   operation for this representation.
            /// - fails: returns [`ArithmeticError::ZeroDivisor`] for a zero
            ///   right operand in division or remainder, and
            ///   [`ArithmeticError::Overflow`] for every other unrepresentable
            ///   result, each naming the operation.
            /// - panics: none.
            ///
            /// # Errors
            /// - [`ArithmeticError::ZeroDivisor`]: division or remainder
            ///   received a zero right operand.
            /// - [`ArithmeticError::Overflow`]: the exact result is outside this
            ///   representation's bounds, signed `MIN / -1` and `MIN % -1`
            ///   included.
            #[spec(ensures: |output| {
                let left = <$representation>::from(self);
                let right = <$representation>::from(rhs);
                let expected = match operation {
                    Operation::Add => left.checked_add(right),
                    Operation::Sub => left.checked_sub(right),
                    Operation::Mul => left.checked_mul(right),
                    Operation::Div => left.checked_div(right),
                    Operation::Rem => left.checked_rem(right),
                };
                match expected {
                    Some(value) => output == Ok(Self::from(value)),
                    None if matches!(operation, Operation::Div | Operation::Rem) && right == <$representation>::default() =>
                        output == Err(ArithmeticError::ZeroDivisor(operation)),
                    None => output == Err(ArithmeticError::Overflow(operation)),
                }
            })]
            #[inline]
            fn checked(self, rhs: Self, operation: Operation) -> Result<Self, ArithmeticError> {
                let result = match operation {
                    Operation::Add => self.0.checked_add(rhs.0),
                    Operation::Sub => self.0.checked_sub(rhs.0),
                    Operation::Mul => self.0.checked_mul(rhs.0),
                    Operation::Div => self.0.checked_div(rhs.0),
                    Operation::Rem => self.0.checked_rem(rhs.0),
                };
                match result {
                    Some(value) => Ok(Self(value)),
                    None => {
                        if matches!(operation, Operation::Div | Operation::Rem) && rhs.0 == <$representation>::default() {
                            Err(ArithmeticError::ZeroDivisor(operation))
                        } else {
                            Err(ArithmeticError::Overflow(operation))
                        }
                    }
                }
            }

            /// Overflow belongs to the representation's modular domain.
            ///
            /// # Specification
            /// - ensures: the result equals the primitive `wrapping_*` result
            ///   for the named operation, so signed `MIN / -1` yields `MIN` and
            ///   signed `MIN % -1` yields zero.
            /// - panics: a zero divisor for division or remainder.
            #[spec(ensures: |output| {
                let left = <$representation>::from(self);
                let right = <$representation>::from(rhs);
                <$representation>::from(output) == match operation {
                    Operation::Add => left.wrapping_add(right),
                    Operation::Sub => left.wrapping_sub(right),
                    Operation::Mul => left.wrapping_mul(right),
                    #[expect(clippy::arithmetic_side_effects, reason = "Postconditions run only after the operation has rejected a zero divisor.")]
                    Operation::Div => left.wrapping_div(right),
                    #[expect(clippy::arithmetic_side_effects, reason = "Postconditions run only after the operation has rejected a zero divisor.")]
                    Operation::Rem => left.wrapping_rem(right),
                }
            })]
            #[inline]
            fn wrapping(self, rhs: Self, operation: Operation) -> Self {
                // The selected family promises modular interpretation.
                Self(match operation {
                    Operation::Add => self.0.wrapping_add(rhs.0),
                    Operation::Sub => self.0.wrapping_sub(rhs.0),
                    Operation::Mul => self.0.wrapping_mul(rhs.0),
                    #[expect(clippy::arithmetic_side_effects, reason = "This named primitive border deliberately panics on a zero divisor, as its specification states.")]
                    Operation::Div => self.0.wrapping_div(rhs.0),
                    #[expect(clippy::arithmetic_side_effects, reason = "This named primitive border deliberately panics on a zero divisor, as its specification states.")]
                    Operation::Rem => self.0.wrapping_rem(rhs.0),
                })
            }

            /// Out-of-range mathematical results project onto representation endpoints.
            ///
            /// # Specification
            /// - ensures: addition, subtraction, multiplication, and division
            ///   equal the primitive `saturating_*` result, and remainder is
            ///   exact because its mathematical result always fits.
            /// - panics: a zero divisor for division or remainder.
            #[spec(ensures: |output| {
                let left = <$representation>::from(self);
                let right = <$representation>::from(rhs);
                <$representation>::from(output) == match operation {
                    Operation::Add => left.saturating_add(right),
                    Operation::Sub => left.saturating_sub(right),
                    Operation::Mul => left.saturating_mul(right),
                    #[expect(clippy::arithmetic_side_effects, reason = "Postconditions run only after the operation has rejected a zero divisor.")]
                    Operation::Div => left.saturating_div(right),
                    #[expect(clippy::arithmetic_side_effects, reason = "Remainders fit the representation; the operation has already rejected a zero divisor.")]
                    Operation::Rem => left.wrapping_rem(right),
                }
            })]
            #[inline]
            fn saturating(self, rhs: Self, operation: Operation) -> Self {
                // The selected family promises interval projection.
                Self(match operation {
                    Operation::Add => self.0.saturating_add(rhs.0),
                    Operation::Sub => self.0.saturating_sub(rhs.0),
                    Operation::Mul => self.0.saturating_mul(rhs.0),
                    #[expect(clippy::arithmetic_side_effects, reason = "This named primitive border deliberately panics on a zero divisor, as its specification states.")]
                    Operation::Div => self.0.saturating_div(rhs.0),
                    // Every defined remainder is representable; MIN % -1 uses zero.
                    #[expect(clippy::arithmetic_side_effects, reason = "This named primitive border deliberately panics on a zero divisor, as its specification states.")]
                    Operation::Rem => self.0.wrapping_rem(rhs.0),
                })
            }

            /// A checked-validity proof admits the representation's unchecked path.
            ///
            /// # Specification
            /// - provides: the primitive unchecked result for addition,
            ///   subtraction, and multiplication, and the unwrapped
            ///   `checked_div` or `checked_rem` value for division and
            ///   remainder, which core exposes no unchecked form for.
            /// - unsafe invariants: the corresponding [`Integer::checked`] call
            ///   returns `Ok`, which the caller has established.
            /// - panics: none.
            ///
            /// # Safety
            /// The corresponding [`Integer::checked`] call must return `Ok`.
            /// Signed `MIN / -1` and `MIN % -1` violate the precondition even
            /// though the mathematical remainder is zero.
            #[inline]
            unsafe fn unchecked(self, rhs: Self, operation: Operation) -> Self {
                Self(match operation {
                    // SAFETY: the caller proves the corresponding checked operation succeeds.
                    Operation::Add => unsafe { self.0.unchecked_add(rhs.0) },
                    // SAFETY: the caller proves the corresponding checked operation succeeds.
                    Operation::Sub => unsafe { self.0.unchecked_sub(rhs.0) },
                    // SAFETY: the caller proves the corresponding checked operation succeeds.
                    Operation::Mul => unsafe { self.0.unchecked_mul(rhs.0) },
                    // SAFETY: checked division is Some by the caller's no-zero/no-overflow proof.
                    // Core exposes no unchecked_div method; this adapter has its exact specification.
                    Operation::Div => unsafe { self.0.checked_div(rhs.0).unwrap_unchecked() },
                    // SAFETY: checked remainder is Some by the caller's no-zero/no-overflow proof.
                    Operation::Rem => unsafe { self.0.checked_rem(rhs.0).unwrap_unchecked() },
                })
            }
        }
    )+};
}

integers!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

/// Named arithmetic families share a permanently strict safe-operator boundary.
macro_rules! binary_family {
    (
        $default:ident,
        $strict:ident,
        $wrapping:ident,
        $saturating:ident,
        $trait:ident,
        $method:ident,
        $operation:ident
    ) => {
        #[doc = concat!("Strict ", stringify!($operation), " in every build profile.")]
        /// # Specification
        /// - ensures: returns the exact representable result.
        /// - panics: overflow or a zero divisor, independent of build profile.
        ///
        /// # Adequacy
        /// - hypothesis: L3 ordinary and boundary pairs distinguish each family and
        ///   operand order.
        /// - witness: `arith::tests::u128_boundaries`
        /// - witness: `arith::tests::i8_negative_boundaries`
        #[inline]
        #[quenchant::spec(ensures: |output| left.checked(right, Operation::$operation) == Ok(output))]
        pub fn $strict<T>(
            left: T,
            right: T,
        ) -> T
        where
            T: Integer,
        {
            left.strict(right, Operation::$operation)
        }

        #[doc = concat!("Modular ", stringify!($operation), ".")]
        /// # Specification
        /// - ensures: returns the width-modular result, including signed MIN / -1.
        /// - panics: a zero divisor in division or remainder.
        ///
        /// # Adequacy
        /// - hypothesis: L3 overflow boundaries distinguish wrapping from strict and
        ///   saturation.
        /// - witness: `arith::tests::u128_boundaries`
        /// - witness: `arith::tests::i8_negative_boundaries`
        #[inline]
        #[quenchant::spec(ensures: |output| output == left.wrapping(right, Operation::$operation))]
        pub fn $wrapping<T>(
            left: T,
            right: T,
        ) -> T
        where
            T: Integer,
        {
            left.wrapping(right, Operation::$operation)
        }

        #[doc = concat!("Clamped ", stringify!($operation), ".")]
        /// # Specification
        /// - ensures: returns the exact result clamped to the representation bounds.
        /// - panics: a zero divisor in division or remainder.
        ///
        /// # Adequacy
        /// - hypothesis: L3 upper and lower boundaries distinguish clamps from
        ///   wrapping.
        /// - witness: `arith::tests::u128_boundaries`
        /// - witness: `arith::tests::i8_negative_boundaries`
        #[inline]
        #[quenchant::spec(ensures: |output| output == left.saturating(right, Operation::$operation))]
        pub fn $saturating<T>(
            left: T,
            right: T,
        ) -> T
        where
            T: Integer,
        {
            left.saturating(right, Operation::$operation)
        }

        #[cfg(not(feature = "fast"))]
        #[doc = concat!("Default strict ", stringify!($operation), ".")]
        /// # Specification
        /// - ensures: returns the exact representable result.
        /// - panics: overflow or a zero divisor in every profile.
        ///
        /// # Adequacy
        /// - hypothesis: L3 ordinary and invalid boundary inputs witness the default
        ///   strict specification.
        /// - witness: `arith::tests::u128_boundaries`
        /// - witness: `arith::tests::i8_negative_boundaries`
        #[inline]
        #[quenchant::spec(ensures: |output| left.checked(right, Operation::$operation) == Ok(output))]
        pub fn $default<T>(
            left: T,
            right: T,
        ) -> T
        where
            T: Integer,
        {
            left.strict(right, Operation::$operation)
        }

        #[cfg(feature = "fast")]
        #[doc = concat!("Unchecked default ", stringify!($operation), ".")]
        /// # Specification
        /// - requires: the corresponding checked operation succeeds.
        /// - ensures: returns the strict result on every valid input.
        /// - panics: none.
        ///
        /// # Safety
        /// The exact primitive operation must be representable, with a nonzero
        /// divisor for division and remainder. Signed MIN / -1 and MIN % -1 are
        /// invalid.
        ///
        /// # Adequacy
        /// - hypothesis: L3 valid boundary pairs witness the result; Miri checks the
        ///   unchecked precondition.
        /// - witness: `arith::tests::u128_boundaries`
        /// - witness: `arith::tests::i8_negative_boundaries`
        #[inline]
        #[quenchant::spec(
            requires: left.checked(right, Operation::$operation).is_ok(),
            ensures: |output| left.checked(right, Operation::$operation) == Ok(output),
        )]
        pub unsafe fn $default<T>(
            left: T,
            right: T,
        ) -> T
        where
            T: Integer,
        {
            // SAFETY: the function's caller proves this exact operation succeeds.
            unsafe { left.unchecked(right, Operation::$operation) }
        }

        impl<Representation> core::ops::$trait for Int<Representation>
        where
            Self: Integer,
        {
            type Output = Self;

            /// Safe operator syntax retains strictness even when `fast` is enabled.
            ///
            /// # Specification
            /// - provides: the strict family's result for the named operation,
            ///   independent of the `fast` feature.
            /// - panics: overflow, or a zero divisor for division or remainder,
            ///   in every profile.
            #[inline]
            fn $method(
                self,
                rhs: Self,
            ) -> Self
            {
                self.strict(rhs, Operation::$operation)
            }
        }
    };
}

binary_family!(add, strict_add, wrapping_add, saturating_add, Add, add, Add);
binary_family!(sub, strict_sub, wrapping_sub, saturating_sub, Sub, sub, Sub);
binary_family!(mul, strict_mul, wrapping_mul, saturating_mul, Mul, mul, Mul);
binary_family!(div, strict_div, wrapping_div, saturating_div, Div, div, Div);
binary_family!(rem, strict_rem, wrapping_rem, saturating_rem, Rem, rem, Rem);

/// Addition exposes an unrepresentable sum as a boundary failure.
///
/// # Specification
/// - ensures: returns the exact sum when representable.
/// - fails: returns [`ArithmeticError::Overflow`] naming [`Operation::Add`].
/// - panics: none.
///
/// # Errors
/// Returns overflow when the mathematical sum is outside the representation.
///
/// # Adequacy
/// - hypothesis: L3 MAX plus one, MIN plus zero, and ordinary unequal operands
///   distinguish failure and value.
/// - witness: `arith::tests::u128_boundaries`
/// - witness: `arith::tests::i8_negative_boundaries`
#[inline]
#[quenchant::spec(ensures: |output| output == left.checked(right, Operation::Add))]
pub fn checked_add<T>(
    left: T,
    right: T,
) -> Result<T, ArithmeticError>
where
    T: Integer,
{
    left.checked(right, Operation::Add)
}

/// Subtraction exposes an unrepresentable difference as a boundary failure.
///
/// # Specification
/// - ensures: returns the exact difference when representable.
/// - fails: returns [`ArithmeticError::Overflow`] naming [`Operation::Sub`].
/// - panics: none.
///
/// # Errors
/// Returns overflow when the mathematical difference is outside the
/// representation.
///
/// # Adequacy
/// - hypothesis: L3 MIN minus one and unequal ordinary operands distinguish
///   bounds and operand order.
/// - witness: `arith::tests::u128_boundaries`
/// - witness: `arith::tests::i8_negative_boundaries`
#[inline]
#[quenchant::spec(ensures: |output| output == left.checked(right, Operation::Sub))]
pub fn checked_sub<T>(
    left: T,
    right: T,
) -> Result<T, ArithmeticError>
where
    T: Integer,
{
    left.checked(right, Operation::Sub)
}

/// Multiplication exposes an unrepresentable product as a boundary failure.
///
/// # Specification
/// - ensures: returns the exact product when representable.
/// - fails: returns [`ArithmeticError::Overflow`] naming [`Operation::Mul`].
/// - panics: none.
///
/// # Errors
/// Returns overflow when the mathematical product is outside the
/// representation.
///
/// # Adequacy
/// - hypothesis: L3 MAX times two, zero, and ordinary unequal operands
///   distinguish overflow and value.
/// - witness: `arith::tests::u128_boundaries`
/// - witness: `arith::tests::i8_negative_boundaries`
#[inline]
#[quenchant::spec(ensures: |output| output == left.checked(right, Operation::Mul))]
pub fn checked_mul<T>(
    left: T,
    right: T,
) -> Result<T, ArithmeticError>
where
    T: Integer,
{
    left.checked(right, Operation::Mul)
}

/// Division retains the distinction between a zero divisor and quotient
/// overflow.
///
/// # Specification
/// - ensures: returns the representable quotient, truncated toward zero.
/// - fails: returns [`ArithmeticError::ZeroDivisor`] or
///   [`ArithmeticError::Overflow`] naming [`Operation::Div`].
/// - panics: none.
///
/// # Errors
/// Zero right operands are zero-divisor failures; signed MIN / -1 is overflow.
///
/// # Adequacy
/// - hypothesis: L3 zero divisors, signed MIN / -1, and nonintegral ordinary
///   quotients distinguish each result.
/// - witness: `arith::tests::u128_boundaries`
/// - witness: `arith::tests::i8_negative_boundaries`
#[inline]
#[quenchant::spec(ensures: |output| output == left.checked(right, Operation::Div))]
pub fn checked_div<T>(
    left: T,
    right: T,
) -> Result<T, ArithmeticError>
where
    T: Integer,
{
    left.checked(right, Operation::Div)
}

/// Remainder retains primitive validity rules, including the signed MIN/-1
/// boundary.
///
/// # Specification
/// - ensures: returns the primitive truncating remainder when defined.
/// - fails: returns [`ArithmeticError::ZeroDivisor`] or
///   [`ArithmeticError::Overflow`] naming [`Operation::Rem`].
/// - panics: none.
///
/// # Errors
/// Zero right operands are zero-divisor failures; signed MIN % -1 is overflow.
///
/// # Adequacy
/// - hypothesis: L3 zero divisors, signed MIN % -1, and nonzero ordinary
///   remainders distinguish each result.
/// - witness: `arith::tests::u128_boundaries`
/// - witness: `arith::tests::i8_negative_boundaries`
#[inline]
#[quenchant::spec(ensures: |output| output == left.checked(right, Operation::Rem))]
pub fn checked_rem<T>(
    left: T,
    right: T,
) -> Result<T, ArithmeticError>
where
    T: Integer,
{
    left.checked(right, Operation::Rem)
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
