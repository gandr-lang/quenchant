//! The calls of a repeated body in a crate that `repeated_call_crates` names:
//! the machinery follows each into its callee. A call is refused when its
//! callee returns a heap value that it allocates and the pass drops it; a
//! call in a function that the loop repeats is refused in that function. The
//! forms beside them are accepted.
#![allow(dead_code, unused_variables)]

/// Every assignment of digits below each base, in order.
struct Odometer
{
    digits: Vec<usize>,
    bases: Vec<usize>,
    done: bool,
}

impl Odometer
{
    fn new(bases: Vec<usize>) -> Self
    {
        let done = bases.contains(&0);
        Self {
            digits: vec![0; bases.len()],
            bases,
            done,
        }
    }

    /// Turn to the next assignment in place; false after the last.
    fn turn(&mut self) -> bool
    {
        for (digit, base) in self.digits.iter_mut().zip(&self.bases) {
            *digit += 1;
            if *digit < *base {
                return true;
            }
            *digit = 0;
        }
        false
    }

    fn reading(&self) -> &[usize]
    {
        &self.digits
    }
}

impl Iterator for Odometer
{
    type Item = Vec<usize>;

    /// A copy of the digits on every step.
    fn next(&mut self) -> Option<Vec<usize>>
    {
        if self.done {
            return None;
        }
        let current = self.digits.clone();
        self.done = !self.turn();
        Some(current)
    }
}

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

/// The image of an assignment, written into the caller's buffer.
fn images_into(
    reading: &[usize],
    images: &mut Vec<u64>,
)
{
    images.clear();
    images.extend(reading.iter().map(|digit| *digit as u64));
}

struct Map
{
    wires: usize,
}

impl Map
{
    /// A fresh mark for each wire.
    fn wire_marks(
        &self,
        images: &[u64],
    ) -> Vec<bool>
    {
        let mut marks = vec![false; self.wires];
        for image in images {
            if let Some(mark) = marks.get_mut(*image as usize) {
                *mark = true;
            }
        }
        marks
    }

    /// Whether every wire is marked; it allocates one call down.
    fn validate(
        &self,
        images: &[u64],
    ) -> bool
    {
        self.wire_marks(images).iter().all(|mark| *mark)
    }
}

/// A value of any type, copied: the copy allocates only as the parameter's
/// `clone` does.
fn copied<T: Clone>(value: &T) -> T
{
    value.clone()
}

/// A refusal; its message allocates on the path that panics.
fn refuse(digit: usize) -> !
{
    panic!("{}", digit.to_string())
}

// Refused: site 1, the `next` of the `for` loop clones the digits; site 2,
// the images are made fresh; site 10, the marks are made fresh in `validate`,
// which the loop repeats.
fn homs(
    map: &Map,
    bases: Vec<usize>,
) -> usize
{
    let mut count = 0;
    for reading in Odometer::new(bases) {
        let images = images_from(&reading, map.wires);
        if map.validate(&images) {
            count += 1;
        }
    }
    count
}

// Accepted: the odometer turns in place, the images grow in one buffer that
// the loop lends, and what a pass keeps leaves the pass.
fn homs_in_place(
    map: &Map,
    bases: Vec<usize>,
) -> Vec<Vec<u64>>
{
    let mut kept = Vec::new();
    let mut images = Vec::new();
    let mut odometer = Odometer::new(bases);
    while odometer.turn() {
        images_into(odometer.reading(), &mut images);
        kept.push(images_from(odometer.reading(), map.wires));
    }
    kept
}

// Accepted: a callee that names a parameter of its caller, and a trait
// object, are callees the build cannot name.
fn unnamed<T: Clone>(
    values: &[T],
    make: &dyn Fn(usize) -> Vec<u64>,
) -> usize
{
    let mut total = 0;
    for (index, value) in values.iter().enumerate() {
        let copy = copied(value);
        total += make(index).len();
    }
    total
}

// Accepted: a call on a path that diverges, a call outside a loop, and a call
// that states its exception.
fn others(
    map: &Map,
    readings: &[Vec<usize>],
) -> usize
{
    let first = images_from(&[], map.wires).len();
    let mut total = first;
    for reading in readings {
        if reading.is_empty() {
            refuse(total);
        }
        #[expect(allocation_in_repeated_body, reason = "the fixture's stated exception")]
        let images = images_from(reading, map.wires);
        total += images.len();
    }
    total
}

fn main()
{
}
