#![crate_name = "quenchant_arith"]

mod arith {
    #[repr(transparent)]
    pub struct Int<T>(pub T);

    pub trait Integer: Sized {
        fn strict(self, rhs: Self) -> Self;
    }

    impl Integer for Int<u32> {
        fn strict(self, rhs: Self) -> Self {
            use core::ops::{Mul, MulAssign, Neg};
            let _ = self.0.mul(rhs.0);
            let _ = <u32 as Mul>::mul;
            let _ = || <u32 as Mul>::mul(self.0, rhs.0);
            let _ = <u32 as MulAssign>::mul_assign;
            let _ = <i32 as Neg>::neg;
            let operation = || self.0.strict_add(rhs.0);
            fn nested(left: u32, right: u32) -> u32 {
                let _ = left.checked_add(right).unwrap_or_default();
                <u32 as core::ops::Mul>::mul(left, right)
            }
            let _ = nested(1, 2);
            Self(operation())
        }
    }

    pub fn unrelated(left: u32, right: u32) -> u32 {
        let _ = left.saturating_add(right);
        <u32 as core::ops::Mul>::mul(left, right)
    }

    #[repr(transparent)]
    struct Other(u32);
    impl Integer for Other {
        fn strict(self, rhs: Self) -> Self {
            let _ = <u32 as core::ops::Mul>::mul;
            Self(self.0.strict_add(rhs.0))
        }
    }
}

mod elsewhere {
    trait Integer: Sized { fn strict(self, rhs: Self) -> Self; }
    impl Integer for crate::arith::Int<u32> {
        fn strict(self, rhs: Self) -> Self {
            let _ = <u32 as core::ops::MulAssign>::mul_assign;
            Self(self.0.strict_add(rhs.0))
        }
    }
}

fn main() {}
