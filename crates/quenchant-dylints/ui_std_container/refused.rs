//! The containers of `alloc` in a crate that the rule lists, and the sites
//! that the rule leaves.

use std::collections::BTreeMap;
use std::sync::Arc;

// Each written container: refused.

struct Held
{
    names: Vec<u8>,
}

fn named(text: &str) -> String
{
    text.to_owned()
}

fn shared(value: u8) -> Arc<u8>
{
    Arc::new(value)
}

fn annotated() -> usize
{
    let items: Vec<u8> = (0_u8 .. 3).collect();
    items.len()
}

// Each call that builds a container with no written type: refused once.

fn built() -> usize
{
    let listed = vec![1_u8, 2];
    let repeated = vec![0_u8; 3];
    let shown = format!("{}", 1_u8);
    let text = 7_u8.to_string();
    let collected = (0_u8 .. 2).collect::<Vec<_>>();
    listed.len() + repeated.len() + shown.len() + text.len() + collected.len()
}

// A map, and a call of a local helper: accepted.

fn mapped() -> BTreeMap<u8, u8>
{
    BTreeMap::new()
}

fn helper() -> usize
{
    named("x").len()
}

// A site that keeps its container and says why: accepted.

#[expect(
    std_container,
    reason = "the boundary that the fixture does not compile"
)]
fn kept() -> Vec<u8>
{
    Vec::new()
}

// An exception the site does not state: refused at the crate root.

#[expect(std_container)]
fn unreasoned() -> Vec<u8>
{
    Vec::new()
}

#[expect(
    std_container,
    reason = "an expectation on a module states no site's exception"
)]
mod inherited
{
    pub fn names() -> Vec<u8>
    {
        Vec::new()
    }
}

#[allow(std_container)]
fn allowed() -> Vec<u8>
{
    Vec::new()
}

fn main()
{
    let held = Held { names: Vec::new() };
    drop(held.names);
    drop(shared(1));
    let _count = annotated() + built() + helper() + mapped().len();
    drop(kept());
    drop(unreasoned());
    drop(inherited::names());
    drop(allowed());
}
