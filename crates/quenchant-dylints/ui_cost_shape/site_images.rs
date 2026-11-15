// Refused: the shape of site 2 of the probe, `images_from` before one image
// buffer served each call of `homs`. Each try of an assignment calls
// `images_from`, which returns a `vec!` it allocates, and the try only reads
// it.
#![allow(dead_code, unused_variables)]

fn images_from(
    assignment: &[usize],
    sources: usize,
) -> Vec<u64>
{
    let mut images = vec![0_u64; sources];
    for (vertex, owner) in assignment.iter().enumerate() {
        if let Some(image) = images.get_mut(*owner) {
            *image |= 1 << vertex;
        }
    }
    images
}

fn plausible(images: &[u64]) -> bool
{
    images.iter().all(|image| *image != 0)
}

fn homs(
    assignments: &[Vec<usize>],
    sources: usize,
) -> usize
{
    let mut found = 0;
    for assignment in assignments {
        let images = images_from(assignment, sources);
        if !plausible(&images) {
            continue;
        }
        found += 1;
    }
    found
}

fn main()
{
}
