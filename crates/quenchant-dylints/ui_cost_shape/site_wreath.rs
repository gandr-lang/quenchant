// The shape of site 8 of the probe, `SiteMap::clone` in `maps_between` before
// a wreath map named its maps by their indices.
#![allow(dead_code, unused_variables)]

use std::collections::BTreeMap;

#[derive(Clone)]
struct SiteMap
{
    images: Box<[u64]>,
}

struct WreathMap
{
    outer: SiteMap,
    columns: Box<[usize]>,
}

// Refused: each pass of the inner loop stores a copy of the outer map, which
// the loop does not change, so the result holds equal copies of one map.
fn maps_between(
    outer: &[SiteMap],
    choices: usize,
) -> Vec<WreathMap>
{
    let mut found = Vec::new();
    for map in outer {
        let mut choice = 0;
        loop {
            found.push(WreathMap {
                outer: map.clone(),
                columns: vec![choice].into_boxed_slice(),
            });
            choice += 1;
            if choice >= choices {
                break;
            }
        }
    }
    found
}

// Accepted: the copy that a closure returns to a map's entry. The entry runs
// the closure once for each new key, and the pass changes the copy after.
fn named(
    labels: &[(u32, String)],
    fallback: &String,
) -> BTreeMap<u32, String>
{
    let mut named = BTreeMap::new();
    for (key, label) in labels {
        named
            .entry(*key)
            .or_insert_with(|| fallback.clone())
            .push_str(label);
    }
    named
}

fn main()
{
}
