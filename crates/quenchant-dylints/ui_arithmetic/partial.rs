#![feature(int_roundings, funnel_shifts)]

type Representation = u32;

fn partial(value: Representation, signed: i32, divisor: u32) {
    let _ = value.pow(2);
    let _ = signed.abs();
    let _ = value.div_euclid(divisor);
    let _ = signed.rem_euclid(2);
    let _ = signed.div_floor(2);
    let _ = value.div_ceil(divisor);
    let _ = value.next_multiple_of(divisor);
    let _ = value.next_power_of_two();
    let _ = value.ilog(divisor);
    let _ = signed.ilog2();
    let _ = value.ilog10();
    let _ = signed.isqrt();
    let _ = value.funnel_shl(divisor, 1);
    let _ = value.funnel_shr(divisor, 1);
    let _ = unsafe { value.unchecked_funnel_shl(divisor, 1) };
    let _ = unsafe { value.unchecked_funnel_shr(divisor, 1) };
    let _ = Representation::pow(value, 2);
    let operation = i32::isqrt;
    let _ = operation(signed);
    let borrowed = &value;
    let _ = borrowed.pow(2);
}

#[repr(transparent)]
pub struct Operand(pub u32);
pub fn power(value: Operand) -> Operand {
    Operand(value.0.pow(2))
}
pub fn multiplication(value: Operand) -> Operand {
    Operand(value.0 * value.0)
}

fn main() {}
