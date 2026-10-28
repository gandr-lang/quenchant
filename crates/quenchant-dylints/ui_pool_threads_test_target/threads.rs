//! A test target owns the threads it races, and still fulfils the
//! expectation a library site states.

fn raced()
{
    drop(std::thread::spawn(|| ()).join());
    rayon::scope(|_| ());
}

// A library site that owns its thread: its expectation is fulfilled in the
// test build too, so no unfulfilled-expectation warning appears.
#[expect(pool_only_threads, reason = "the world thread owns its runtime")]
fn world()
{
    drop(std::thread::spawn(|| ()).join());
}

#[test]
fn races()
{
    raced();
    world();
}
