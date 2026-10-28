//! Threads and pools started outside a consumer's work pool.

// Each call that starts a thread or a pool: refused.

fn spawned()
{
    drop(std::thread::spawn(|| ()).join());
}

fn scoped()
{
    std::thread::scope(|scope| {
        // A spawn inside a scope starts nothing the scope did not.
        drop(scope.spawn(|| ()));
    });
}

fn built()
{
    drop(std::thread::Builder::new().spawn(|| ()));
    std::thread::scope(|scope| {
        drop(std::thread::Builder::new().spawn_scoped(scope, || ()));
    });
}

fn pooled()
{
    drop(rayon::ThreadPoolBuilder::new().num_threads(2).build());
}

fn forked()
{
    rayon::scope(|_| ());
    rayon::in_place_scope(|_| ());
    rayon::spawn(|| ());
    let _joined = rayon::join(|| 1_u8, || 2_u8);
}

// Calls that start nothing: accepted.

fn read()
{
    drop(std::thread::current());
    let _width = rayon::current_num_threads();
}

// A site that owns its thread and says why: accepted.

#[expect(pool_only_threads, reason = "the world thread owns its runtime")]
fn world()
{
    drop(std::thread::spawn(|| ()).join());
}

fn listener()
{
    #[expect(pool_only_threads, reason = "the listener blocks on its socket")]
    let reader = std::thread::spawn(|| ());
    drop(reader.join());
}

// An exception the site does not state: refused at the crate root.

#[expect(pool_only_threads)]
fn unreasoned()
{
    drop(std::thread::spawn(|| ()).join());
}

#[expect(
    pool_only_threads,
    reason = "a module's expectation excepts nothing inside it"
)]
mod inherited
{
    pub fn reader()
    {
        drop(std::thread::spawn(|| ()).join());
    }
}

#[allow(pool_only_threads)]
fn allowed()
{
    drop(std::thread::spawn(|| ()).join());
}

fn main()
{
    spawned();
    scoped();
    built();
    pooled();
    forked();
    read();
    world();
    listener();
    unreasoned();
    inherited::reader();
    allowed();
}
