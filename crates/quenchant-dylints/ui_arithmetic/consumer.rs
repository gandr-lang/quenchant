use quenchant_arith::arith::{self, ArithmeticError, Int};
use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    mod lookup {
        pub enum Missing { NotStored }
    }
}

fn arithmetic(left: Int<u32>, right: Int<u32>) -> Result<Int<u32>, ArithmeticError> {
    let strict = arith::strict_add(left, right);
    let _ = left + right;
    // The checksum relation is modular arithmetic.
    let _ = arith::wrapping_mul(left, right);
    // A remaining-budget floor is part of this operation's meaning.
    let _ = arith::saturating_sub(left, right);
    arith::checked_div(strict, right)
}

fn absent() -> Maybe<Int<u32>, lookup::Missing> {
    Maybe::Absent(lookup::Missing::NotStored)
}

fn main() {}
