// Refused: the shape of site 1 of the probe, `Odometer::next` before it lent
// its digits. Each pass of the loop in `homs` takes a clone of the odometer's
// digits, which `next` allocates, and the pass drops it.
#![allow(dead_code, unused_variables)]

struct Odometer
{
    bases: Vec<usize>,
    digits: Vec<usize>,
    done: bool,
}

impl Odometer
{
    fn new(bases: Vec<usize>) -> Self
    {
        let digits = vec![0; bases.len()];
        Self {
            bases,
            digits,
            done: false,
        }
    }
}

fn advance(
    digits: &mut [usize],
    bases: &[usize],
) -> bool
{
    for (digit, base) in digits.iter_mut().zip(bases) {
        *digit += 1;
        if *digit < *base {
            return false;
        }
        *digit = 0;
    }
    true
}

impl Iterator for Odometer
{
    type Item = Vec<usize>;

    fn next(&mut self) -> Option<Vec<usize>>
    {
        if self.done {
            return None;
        }
        self.done = advance(&mut self.digits, &self.bases);
        Some(self.digits.clone())
    }
}

fn homs(
    bases: Vec<usize>,
    sources: usize,
) -> usize
{
    let mut found = 0;
    for assignment in Odometer::new(bases) {
        if assignment.iter().sum::<usize>() == sources {
            found += 1;
        }
    }
    found
}

fn main()
{
}
