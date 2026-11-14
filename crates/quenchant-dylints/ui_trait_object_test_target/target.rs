//! A test target reads only the trait objects whose sites state an
//! expectation.

use std::error::Error;

// A library site that states its exception: the expectation holds in the
// test build too.

#[expect(trait_object, reason = "std's own signature of `Error::source`")]
fn source(error: &dyn Error) -> Option<&(dyn Error + 'static)>
{
    error.source()
}

// A site that states no expectation: not read in a test target.

fn boxed() -> Result<(), Box<dyn Error>>
{
    Ok(())
}

#[test]
fn reads()
{
    let held: Box<dyn Error> = "text".into();
    assert!(boxed().is_ok());
    assert!(source(&*held).is_none());
}
