#[repr(transparent)]
#[derive(Clone, Copy)]
struct Count(u32);

impl Count {
    fn checked_add(self, _other: Self) -> Self { self }
    fn wrapping_mul(self, _other: Self) -> Self { self }
    fn saturating_sub(self, _other: Self) -> Self { self }
    fn pow(self, _exponent: u32) -> Self { self }
    fn isqrt(self) -> Self { self }
}

impl core::ops::Add for Count {
    type Output = Self;
    fn add(self, _other: Self) -> Self { self }
}

trait IntegerExtension {
    fn checked_add(self, other: Self) -> Self;
    fn pow(self, exponent: u32) -> Self;
}
impl IntegerExtension for u32 {
    fn checked_add(self, _other: Self) -> Self { self }
    fn pow(self, _exponent: u32) -> Self { self }
}
fn accepted(left: Count, right: Count, bits: u32) {
    let _ = left + right;
    let _ = left.checked_add(right);
    let _ = Count::wrapping_mul(left, right);
    let operation = Count::saturating_sub;
    let _ = operation(left, right);
    let _ = bits & 1;
    let _ = bits == 1;
    let _ = !bits;
    let _ = <u32 as IntegerExtension>::checked_add(bits, bits);
    let _ = left.pow(2);
    let operation = Count::isqrt;
    let _ = operation(left);
    let _ = <u32 as IntegerExtension>::pow(bits, 2);
    let _ = bits.isqrt();
    let _ = bits.abs_diff(1);
    let _ = bits.midpoint(1);
    let _ = i32::MIN.unsigned_abs();
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Delta(u32);
impl core::ops::Add<Delta> for u32 {
    type Output = Count;
    fn add(self, rhs: Delta) -> Count { Count(rhs.0) }
}
impl core::ops::AddAssign<Delta> for u32 {
    fn add_assign(&mut self, rhs: Delta) { *self = rhs.0; }
}
fn nominal_rhs(mut value: u32, delta: Delta) {
    let _ = value + delta;
    value += delta;
}

fn resolved_nominal(mut value: u32, delta: Delta, count: Count) {
    use core::ops::{Add, AddAssign};
    let _ = value.add(delta);
    let _ = <u32 as Add<Delta>>::add(value, delta);
    let _ = <u32 as Add<Delta>>::add;
    value.add_assign(delta);
    <u32 as AddAssign<Delta>>::add_assign(&mut value, delta);
    let _ = <u32 as AddAssign<Delta>>::add_assign;
    let _ = count.add(count);
    let _ = <Count as Add>::add;
}

trait Mul {
    fn mul(self, rhs: Self) -> Self;
}
impl Mul for u32 {
    fn mul(self, _rhs: Self) -> Self { self }
}
fn unrelated_operator(value: u32) {
    let _ = value.mul(value);
    let _ = <u32 as Mul>::mul(value, value);
    let _ = <u32 as Mul>::mul;
}

fn non_arithmetic(mut value: u32, rhs: u32) {
    use core::ops::{BitAnd, BitOr, BitXor, BitAndAssign, BitOrAssign, BitXorAssign, Not};
    let _ = value.bitand(rhs);
    let _ = <u32 as BitOr>::bitor(value, rhs);
    let _ = <u32 as BitXor>::bitxor;
    value.bitand_assign(rhs);
    <u32 as BitOrAssign>::bitor_assign(&mut value, rhs);
    let _ = <u32 as BitXorAssign>::bitxor_assign;
    let _ = value.not();
    let _ = <u32 as Not>::not;
    let _ = <u32 as PartialEq>::eq(&value, &rhs);
}

fn generic<T: core::ops::Mul<Output = T> + Copy>(value: T) -> T {
    let _ = <T as core::ops::Mul>::mul;
    value.mul(value)
}
fn generic_rhs<T>(value: u32, rhs: T) -> <u32 as core::ops::Add<T>>::Output
where u32: core::ops::Add<T> {
    <u32 as core::ops::Add<T>>::add(value, rhs)
}

fn floating(value: f32) {
    use core::ops::Mul;
    let _ = value.mul(value);
    let _ = <f32 as Mul>::mul;
}

impl core::ops::Add<&Delta> for &u32 {
    type Output = Count;
    fn add(self, rhs: &Delta) -> Count { Count(rhs.0) }
}
impl core::ops::AddAssign<&Delta> for u32 {
    fn add_assign(&mut self, rhs: &Delta) { *self = rhs.0; }
}
fn borrowed_nominal(mut value: u32, delta: &Delta) {
    use core::ops::{Add, AddAssign};
    let _ = (&value).add(delta);
    let _ = <&u32 as Add<&Delta>>::add;
    value.add_assign(delta);
    let _ = <u32 as AddAssign<&Delta>>::add_assign;
}

trait Domain { type Value; }
fn unresolved_projection<T: Domain>(value: T::Value)
where T::Value: core::ops::Mul<Output = T::Value> + Copy {
    let _ = <T::Value as core::ops::Mul>::mul(value, value);
    let _ = <T::Value as core::ops::Mul>::mul;
}
fn main() {}
