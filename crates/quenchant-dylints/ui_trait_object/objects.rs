//! Trait objects of the configured classes `Error` and `Fn`, and of `Display`,
//! which no class takes.

use std::error::Error;
use std::fmt::Display;

// Each trait object of a configured class: refused.

fn boxed() -> Result<(), Box<dyn Error>>
{
    Ok(())
}

fn borrowed(error: &dyn Error) -> usize
{
    error.to_string().len()
}

fn sent(errors: Vec<Box<dyn Error + Send>>) -> usize
{
    errors.len()
}

fn called(callback: &dyn Fn(u8) -> u8) -> u8
{
    callback(1)
}

// A trait object of a class that is not configured: accepted.

fn shown(value: &dyn Display) -> String
{
    value.to_string()
}

// A site that keeps its trait object and says why: accepted.

#[expect(trait_object, reason = "std's own signature of `Error::source`")]
fn source(error: &dyn Error) -> Option<&(dyn Error + 'static)>
{
    error.source()
}

fn statement() -> usize
{
    #[expect(
        trait_object,
        reason = "the boundary that the fixture does not compile"
    )]
    let held: Box<dyn Error> = "text".into();
    held.to_string().len()
}

// An exception the site does not state: refused at the crate root.

#[expect(trait_object)]
fn unreasoned() -> Result<(), Box<dyn Error>>
{
    Ok(())
}

#[expect(
    trait_object,
    reason = "an expectation on a module states no site's exception"
)]
mod inherited
{
    pub fn boxed() -> Result<(), Box<dyn std::error::Error>>
    {
        Ok(())
    }
}

#[allow(trait_object)]
fn allowed() -> Result<(), Box<dyn Error>>
{
    Ok(())
}

fn main()
{
    drop(boxed());
    let _length = borrowed(&std::fmt::Error);
    let _count = sent(Vec::new());
    let _byte = called(&|byte| byte);
    drop(shown(&1));
    let _cause = source(&std::fmt::Error);
    let _held = statement();
    drop(unreasoned());
    drop(inherited::boxed());
    drop(allowed());
}
