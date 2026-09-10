use core::ops::{Add, Sub, Mul, Div, Rem, Shl, Shr, AddAssign, SubAssign, MulAssign, DivAssign, RemAssign, ShlAssign, ShrAssign, Neg};

type Word = u32;

fn methods(mut value: Word, rhs: Word, signed: i32) {
    let _ = value.add(rhs);
    let _ = value.sub(rhs);
    let _ = value.mul(rhs);
    let _ = value.div(rhs);
    let _ = value.rem(rhs);
    let _ = value.shl(rhs);
    let _ = value.shr(rhs);
    value.add_assign(rhs);
    value.sub_assign(rhs);
    value.mul_assign(rhs);
    value.div_assign(rhs);
    value.rem_assign(rhs);
    value.shl_assign(rhs);
    value.shr_assign(rhs);
    let _ = signed.neg();
}

fn ufcs(mut value: Word, rhs: Word, signed: i32) {
    let _ = <Word as Add>::add(value, rhs);
    let _ = <Word as Sub>::sub(value, rhs);
    let _ = <u32 as core::ops::Mul>::mul(value, rhs);
    let _ = <Word as Div>::div(value, rhs);
    let _ = <Word as Rem>::rem(value, rhs);
    let _ = <Word as Shl>::shl(value, rhs);
    let _ = <Word as Shr>::shr(value, rhs);
    <Word as AddAssign>::add_assign(&mut value, rhs);
    <Word as SubAssign>::sub_assign(&mut value, rhs);
    <Word as MulAssign>::mul_assign(&mut value, rhs);
    <Word as DivAssign>::div_assign(&mut value, rhs);
    <Word as RemAssign>::rem_assign(&mut value, rhs);
    <Word as ShlAssign>::shl_assign(&mut value, rhs);
    <Word as ShrAssign>::shr_assign(&mut value, rhs);
    let _ = <i32 as Neg>::neg(signed);
}

fn items() {
    let _ = <Word as Add>::add;
    let _ = <Word as Sub>::sub;
    let _ = <Word as Mul>::mul;
    let _ = <Word as Div>::div;
    let _ = <Word as Rem>::rem;
    let _ = <Word as Shl>::shl;
    let _ = <Word as Shr>::shr;
    let _ = <Word as AddAssign>::add_assign;
    let _ = <Word as SubAssign>::sub_assign;
    let _ = <Word as MulAssign>::mul_assign;
    let _ = <Word as DivAssign>::div_assign;
    let _ = <Word as RemAssign>::rem_assign;
    let _ = <Word as ShlAssign>::shl_assign;
    let _ = <Word as ShrAssign>::shr_assign;
    let _ = <i32 as Neg>::neg;
}

fn borrowed(mut value: u32, rhs: u32, signed: i32) {
    let _ = <&u32 as Mul<&u32>>::mul(&value, &rhs);
    let _ = (&value).mul(&rhs);
    let _ = <u32 as Mul<&u32>>::mul;
    let _ = <&u32 as Mul<u32>>::mul;
    value.add_assign(&rhs);
    let _ = <u32 as AddAssign<&u32>>::add_assign;
    let _ = <&i32 as Neg>::neg(&signed);
    let _ = <&i32 as Neg>::neg;
    let _ = <u32 as Shl<u8>>::shl(value, 1);
    let _ = <u32 as ShrAssign<u8>>::shr_assign;
    let operation: fn(u32, u32) -> u32 = Mul::mul;
    let _ = operation(value, rhs);
}

trait Domain { type Value; }
fn resolved_projection<T: Domain<Value = u32>>(value: T::Value) {
    let _ = <T::Value as Mul>::mul(value, value);
    let _ = <T::Value as Mul>::mul;
}

#[repr(transparent)]
struct Borrowed(u32);
impl core::ops::Deref for Borrowed {
    type Target = u32;
    fn deref(&self) -> &u32 { &self.0 }
}
fn adjusted(value: Borrowed) {
    let _ = value.mul(1);
}

fn main() {}
