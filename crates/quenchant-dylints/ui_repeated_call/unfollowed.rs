//! The calls of a repeated body in a crate that `repeated_call_crates` does
//! not name: the lint follows no call, so the callee's fresh vector on every
//! pass is not refused.
#![allow(dead_code, unused_variables)]

/// A fresh image for each assignment.
fn images_from(
    reading: &[usize],
    width: usize,
) -> Vec<u64>
{
    let mut images = vec![0; width];
    for (image, digit) in images.iter_mut().zip(reading) {
        *image = *digit as u64;
    }
    images
}

// Accepted: the crate is not followed.
fn homs(readings: &[Vec<usize>]) -> usize
{
    let mut total = 0;
    for reading in readings {
        total += images_from(reading, 4).len();
    }
    total
}

fn main()
{
}
