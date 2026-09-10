#![crate_name = "quenchant_arith"]

#[path = "../../quenchant-arith/src/arith.rs"]
mod arith;

fn main() {
    let left = arith::Int::from(2_u32);
    let right = arith::Int::from(3_u32);
    let _ = arith::strict_add(left, right);
    let _ = arith::checked_sub(right, left);
    let _ = arith::wrapping_mul(left, right);
    let _ = arith::saturating_div(right, left);
}
