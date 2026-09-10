type Representation = u32;

fn primitive(mut left: Representation, right: u32) {
    let _ = left + right;
    let _ = left - right;
    let _ = left * right;
    let _ = left / right;
    let _ = left % right;
    let _ = left << right;
    left += right;
    left >>= right;
    let _ = -i32::from(1_i8);
    let _ = left.checked_add(right);
    let _ = left.saturating_sub(right);
    let _ = left.wrapping_mul(right);
    let _ = left.strict_div(right);
    let _ = left.overflowing_rem(right);
    let _ = unsafe { left.unchecked_add(right) };
    let _ = Representation::checked_add(left, right);
    let _ = <u32>::saturating_sub(left, right);
    let operation = u32::wrapping_mul;
    let _ = operation(left, right);
    let borrowed = &left;
    let _ = borrowed.checked_add(right);
    let _ = left.checked_shl(right);
    let _ = left.checked_div_euclid(right);
}

#[repr(transparent)]
struct Borrowed(u32);
impl core::ops::Deref for Borrowed {
    type Target = u32;
    fn deref(&self) -> &u32 { &self.0 }
}
fn adjusted_receiver(value: Borrowed) {
    let _ = value.checked_add(1);
}

macro_rules! primitive_call {
    ($value:expr) => { $value.checked_add(1) };
}
fn expanded(value: u32) {
    let _ = primitive_call!(value);
}

fn main() {}
