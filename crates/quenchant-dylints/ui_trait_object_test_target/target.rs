//! A test target reads each trait object as a library does: a site that
//! states no expectation is refused, in a test function and under
//! `#[cfg(test)]` too.

use std::error::Error;

// A library site that states its exception: the expectation holds in the
// test build too.

#[expect(trait_object, reason = "std's own signature of `Error::source`")]
fn source(error: &dyn Error) -> Option<&(dyn Error + 'static)>
{
    error.source()
}

// A library site that states no expectation: refused in its test build too.

fn boxed() -> Result<(), Box<dyn Error>>
{
    Ok(())
}

// A test function's trait object: refused.

#[test]
fn reads()
{
    let held: Box<dyn Error> = "text".into();
    assert!(boxed().is_ok());
    assert!(support::failing().is_ok());
    assert!(source(&*held).is_none());
}

// A trait object under `#[cfg(test)]`: refused.

#[cfg(test)]
mod support
{
    pub fn failing() -> Result<(), Box<dyn std::error::Error>>
    {
        Ok(())
    }
}
