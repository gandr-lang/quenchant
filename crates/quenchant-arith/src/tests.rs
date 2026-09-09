//! Observable arithmetic specifications across every representation width.

use super::ArithmeticError;
use super::Int;
use super::Operation;
use super::add;
use super::checked_add;
use super::checked_div;
use super::checked_mul;
use super::checked_rem;
use super::checked_sub;
use super::div;
use super::mul;
use super::rem;
use super::saturating_add;
use super::saturating_div;
use super::saturating_mul;
use super::saturating_rem;
use super::saturating_sub;
use super::strict_add;
use super::strict_div;
use super::strict_mul;
use super::strict_rem;
use super::strict_sub;
use super::sub;
use super::wrapping_add;
use super::wrapping_div;
use super::wrapping_mul;
use super::wrapping_rem;
use super::wrapping_sub;

/// Boundary-biased operand pairs compare each family with its primitive model.
macro_rules! family_grid {
    (
        $inputs:ident,
        $zero:ident,
        $operation:ident,
        $default:ident,
        $strict:ident,
        $checked:ident,
        $wrapping:ident,
        $saturating:ident,
        $saturation_model:ident
    ) => {
        for left in $inputs {
            for right in $inputs {
                let operation = Operation::$operation;
                let zero_divisor =
                    matches!(operation, Operation::Div | Operation::Rem) && right == $zero;
                let expected = left.$checked(right).map(Int::from).ok_or(if zero_divisor {
                    ArithmeticError::ZeroDivisor(operation)
                }
                else {
                    ArithmeticError::Overflow(operation)
                });
                let lhs = Int::from(left);
                let rhs = Int::from(right);
                assert_eq!($checked(lhs, rhs), expected);
                if !zero_divisor {
                    // These model calls intentionally inhabit modular and clamped domains.
                    assert_eq!($wrapping(lhs, rhs), Int::from(left.$wrapping(right)));
                    assert_eq!(
                        $saturating(lhs, rhs),
                        Int::from(left.$saturation_model(right))
                    );
                }
                if let Ok(exact) = expected {
                    assert_eq!($strict(lhs, rhs), exact);
                    #[cfg(not(feature = "fast"))]
                    assert_eq!($default(lhs, rhs), exact);
                    #[cfg(feature = "fast")]
                    {
                        // SAFETY: the primitive specification, not the implementation under
                        // test, just proved this exact operation is defined and representable.
                        unsafe {
                            assert_eq!($default(lhs, rhs), exact);
                        }
                    }
                }
            }
        }
    };
}

/// Generate width-specific specifications without casts or host-width
/// assumptions.
macro_rules! width_specification {
    ($name:ident, $representation:ty) => {
        #[test]
        fn $name()
        {
            let zero_representation: $representation = 0;
            let one_representation: $representation = 1;
            let two_representation: $representation = 2;
            let zero = Int::from(zero_representation);
            let one = Int::from(one_representation);
            let two = Int::from(two_representation);
            let max = Int::<$representation>::MAX;
            let min = Int::<$representation>::MIN;
            let inputs: [$representation; 5] = [
                <$representation>::MIN,
                <$representation>::MIN.strict_add(1),
                7,
                <$representation>::MAX.strict_sub(1),
                <$representation>::MAX,
            ];
            family_grid!(
                inputs,
                zero_representation,
                Add,
                add,
                strict_add,
                checked_add,
                wrapping_add,
                saturating_add,
                saturating_add
            );
            family_grid!(
                inputs,
                zero_representation,
                Sub,
                sub,
                strict_sub,
                checked_sub,
                wrapping_sub,
                saturating_sub,
                saturating_sub
            );
            family_grid!(
                inputs,
                zero_representation,
                Mul,
                mul,
                strict_mul,
                checked_mul,
                wrapping_mul,
                saturating_mul,
                saturating_mul
            );
            #[expect(clippy::arithmetic_side_effects, reason = "The reference grid excludes zero divisors before calling the primitive models.")]
            {
            family_grid!(
                inputs,
                zero_representation,
                Div,
                div,
                strict_div,
                checked_div,
                wrapping_div,
                saturating_div,
                saturating_div
            );
            }
            // A defined remainder needs no clamping; the modular model also covers MIN/-1.
            #[expect(clippy::arithmetic_side_effects, reason = "The reference grid excludes zero divisors before calling the primitive models.")]
            {
            family_grid!(
                inputs,
                zero_representation,
                Rem,
                rem,
                strict_rem,
                checked_rem,
                wrapping_rem,
                saturating_rem,
                wrapping_rem
            );
            }

            assert_eq!(
                checked_add(max, one),
                Err(ArithmeticError::Overflow(Operation::Add))
            );
            assert_eq!(
                checked_sub(min, one),
                Err(ArithmeticError::Overflow(Operation::Sub))
            );
            assert_eq!(
                checked_mul(max, two),
                Err(ArithmeticError::Overflow(Operation::Mul))
            );
            assert_eq!(
                checked_div(one, zero),
                Err(ArithmeticError::ZeroDivisor(Operation::Div))
            );
            assert_eq!(
                checked_rem(one, zero),
                Err(ArithmeticError::ZeroDivisor(Operation::Rem))
            );

            // Optimization profile must not change a strict operation's panic boundary.
            assert!(std::panic::catch_unwind(|| strict_add(max, one)).is_err());
            assert!(std::panic::catch_unwind(|| strict_sub(min, one)).is_err());
            assert!(std::panic::catch_unwind(|| strict_mul(max, two)).is_err());
            assert!(std::panic::catch_unwind(|| strict_div(one, zero)).is_err());
            assert!(std::panic::catch_unwind(|| strict_rem(one, zero)).is_err());
            assert!(std::panic::catch_unwind(|| max + one).is_err());
            assert!(std::panic::catch_unwind(|| wrapping_div(one, zero)).is_err());
            assert!(std::panic::catch_unwind(|| wrapping_rem(one, zero)).is_err());
            assert!(std::panic::catch_unwind(|| saturating_div(one, zero)).is_err());
            assert!(std::panic::catch_unwind(|| saturating_rem(one, zero)).is_err());
            #[cfg(not(feature = "fast"))]
            assert!(std::panic::catch_unwind(|| add(max, one)).is_err());
        }
    };
}

/// Signed cases distinguish MIN/-1 overflow from truncation toward zero.
macro_rules! signed_specification {
    ($name:ident, $representation:ty) => {
        #[test]
        fn $name()
        {
            let min = Int::<$representation>::MIN;
            let negative_one: $representation = -1;
            let zero_representation: $representation = 0;
            let negative_seven: $representation = -7;
            let two: $representation = 2;
            let negative_three: $representation = -3;
            let minus_one = Int::from(negative_one);
            let zero = Int::from(zero_representation);
            assert_eq!(
                checked_div(min, minus_one),
                Err(ArithmeticError::Overflow(Operation::Div))
            );
            assert_eq!(
                checked_rem(min, minus_one),
                Err(ArithmeticError::Overflow(Operation::Rem))
            );
            // The two domains intentionally give different answers at quotient overflow.
            assert_eq!(wrapping_div(min, minus_one), min);
            assert_eq!(wrapping_rem(min, minus_one), zero);
            assert_eq!(saturating_div(min, minus_one), Int::<$representation>::MAX);
            assert_eq!(saturating_rem(min, minus_one), zero);
            assert!(std::panic::catch_unwind(|| strict_div(min, minus_one)).is_err());
            assert!(std::panic::catch_unwind(|| strict_rem(min, minus_one)).is_err());
            assert_eq!(
                checked_div(Int::from(negative_seven), Int::from(two)),
                Ok(Int::from(negative_three))
            );
            assert_eq!(
                checked_rem(Int::from(negative_seven), Int::from(two)),
                Ok(minus_one)
            );
        }
    };
}

width_specification!(u8_boundaries, u8);
width_specification!(u16_boundaries, u16);
width_specification!(u32_boundaries, u32);
width_specification!(u64_boundaries, u64);
width_specification!(u128_boundaries, u128);
width_specification!(usize_boundaries, usize);
width_specification!(i8_boundaries, i8);
width_specification!(i16_boundaries, i16);
width_specification!(i32_boundaries, i32);
width_specification!(i64_boundaries, i64);
width_specification!(i128_boundaries, i128);
width_specification!(isize_boundaries, isize);
signed_specification!(i8_negative_boundaries, i8);
signed_specification!(i16_negative_boundaries, i16);
signed_specification!(i32_negative_boundaries, i32);
signed_specification!(i64_negative_boundaries, i64);
signed_specification!(i128_negative_boundaries, i128);
signed_specification!(isize_negative_boundaries, isize);

#[test]
fn diagnostics_distinguish_operations_and_failure_causes()
{
    let operations = [
        Operation::Add,
        Operation::Sub,
        Operation::Mul,
        Operation::Div,
        Operation::Rem,
    ];
    for operation in operations {
        let overflow = std::format!("{}", ArithmeticError::Overflow(operation));
        let zero_divisor = std::format!("{}", ArithmeticError::ZeroDivisor(operation));
        assert_ne!(overflow, zero_divisor);
        for other_operation in operations {
            if operation != other_operation {
                let other = std::format!("{}", ArithmeticError::Overflow(other_operation));
                assert_ne!(overflow, other);
            }
        }
    }
}

/// Enforcement, rather than feature selection alone, determines whether a false
/// predicate panics.
#[cfg(all(feature = "anodized", anodized_panic))]
#[test]
fn specification_enforcement_rejects_false_postcondition()
{
    /// A normally returning body isolates failure in the generated
    /// postcondition.
    ///
    /// # Specification
    /// - ensures: false, deliberately violated to witness enforcement.
    /// - panics: the enforcing postcondition rejects every return.
    ///
    /// # Adequacy
    /// - hypothesis: L3 an empty body cannot produce the asserted postcondition
    ///   failure; missing proc-macro enforcement returns normally instead.
    /// - witness: `arith::tests::specification_enforcement_rejects_false_postcondition`
    #[quenchant::spec(ensures: false)]
    fn rejected()
    {
    }

    let failure = std::panic::catch_unwind(rejected)
        .expect_err("the enforcing lane must reject a false postcondition");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("the backend reports a textual failure");
    assert!(
        message.starts_with("postcondition failed"),
        "unrelated panic: {message}"
    );
}
