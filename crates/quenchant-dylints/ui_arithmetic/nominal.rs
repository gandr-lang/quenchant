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
fn main() {}
