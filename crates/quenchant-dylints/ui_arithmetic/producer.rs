#![crate_name = "quenchant_arith"]

mod arith {
    #[repr(transparent)]
    pub struct Int<T>(pub T);

    pub trait Integer: Sized {
        fn strict(self, rhs: Self) -> Self;
    }

    impl Integer for Int<u32> {
        fn strict(self, rhs: Self) -> Self {
            let operation = || self.0.strict_add(rhs.0);
            fn nested(left: u32, right: u32) -> u32 {
                left.checked_add(right).unwrap_or_default()
            }
            let _ = nested(1, 2);
            Self(operation())
        }
    }

    pub fn unrelated(left: u32, right: u32) -> u32 {
        left.saturating_add(right)
    }

    #[repr(transparent)]
    struct Other(u32);
    impl Integer for Other {
        fn strict(self, rhs: Self) -> Self {
            Self(self.0.strict_add(rhs.0))
        }
    }
}

mod elsewhere {
    trait Integer: Sized { fn strict(self, rhs: Self) -> Self; }
    impl Integer for crate::arith::Int<u32> {
        fn strict(self, rhs: Self) -> Self {
            Self(self.0.strict_add(rhs.0))
        }
    }
}

fn main() {}
