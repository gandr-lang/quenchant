//! A test target owns the threads it races: nothing is refused.

fn raced()
{
    drop(std::thread::spawn(|| ()).join());
    rayon::scope(|_| ());
}

#[test]
fn races()
{
    raced();
}
