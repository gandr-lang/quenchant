// Refused: the shape of site 10 of the probe, `SiteMap::wire_marks` before
// the marks moved into a reused buffer. `homs` validates each candidate map in
// a loop; `validate` calls `wire_marks`, which returns a `vec!` it allocates,
// and `validate` only reads it.
#![allow(dead_code, unused_variables)]

struct SiteMap
{
    wires: Vec<usize>,
}

impl SiteMap
{
    fn wire_marks(
        &self,
        wire_count: usize,
    ) -> Vec<bool>
    {
        let mut marks = vec![false; wire_count];
        for wire in self.wires.iter().copied() {
            if let Some(mark) = marks.get_mut(wire) {
                *mark = true;
            }
        }
        marks
    }

    fn validate(
        &self,
        wire_count: usize,
    ) -> bool
    {
        let marks = self.wire_marks(wire_count);
        marks.iter().all(|mark| *mark)
    }
}

fn homs(
    maps: &[SiteMap],
    wire_count: usize,
) -> usize
{
    let mut valid = 0;
    for map in maps {
        if map.validate(wire_count) {
            valid += 1;
        }
    }
    valid
}

fn main()
{
}
