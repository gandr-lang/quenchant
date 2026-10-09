// aux-build: anyhow.rs
// The erased-error gate, opted into at the crate root.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(erased_error_signature))]

extern crate anyhow;

use core::error::Error;
use core::fmt;
use std::rc::Rc;
use std::sync::Arc;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Count(u32);

/// The failures a load can produce.
#[derive(Debug)]
enum LoadError
{
    Missing,
    Wrapped(Box<dyn Error>),
}

impl fmt::Display for LoadError
{
    fn fmt(
        &self,
        formatter: &mut fmt::Formatter<'_>,
    ) -> fmt::Result
    {
        formatter.write_str("load failed")
    }
}

impl Error for LoadError
{
}

type Fallible<Value> = Result<Value, Box<dyn Error>>;

fn named() -> Result<Count, LoadError>
{
    Err(LoadError::Missing)
}

fn named_with_source() -> Result<Count, LoadError>
{
    Err(LoadError::Wrapped(Box::new(LoadError::Missing)))
}

fn reports(error: Box<dyn Error>) -> LoadError
{
    LoadError::Wrapped(error)
}

fn boxed() -> Result<Count, Box<dyn Error>>
{
    Err(Box::new(LoadError::Missing))
}

fn boxed_send() -> Result<Count, Box<dyn Error + Send + Sync>>
{
    Ok(Count(0))
}

fn shared() -> Result<Count, Arc<dyn Error + Send + Sync>>
{
    Ok(Count(0))
}

fn counted() -> Result<Count, Rc<dyn Error>>
{
    Ok(Count(0))
}

fn borrowed() -> Result<Count, &'static dyn Error>
{
    Ok(Count(0))
}

fn aliased() -> Fallible<Count>
{
    Ok(Count(0))
}

fn nested() -> Vec<Result<Count, Box<dyn Error>>>
{
    Vec::new()
}

async fn later() -> Result<Count, Box<dyn Error>>
{
    Ok(Count(0))
}

fn iterated() -> impl Iterator<Item = Result<Count, Box<dyn Error>>>
{
    core::iter::empty()
}

fn dependency() -> anyhow::Result<Count>
{
    Ok(Count(0))
}

/// A local trait's methods are crate-defined signatures.
trait Source
{
    fn read(&self) -> Result<Count, Box<dyn Error>>;
}

struct Reader;

impl Source for Reader
{
    fn read(&self) -> Result<Count, Box<dyn Error>>
    {
        Ok(Count(0))
    }
}

/// A foreign trait's method answers to that trait's signature.
impl core::str::FromStr for Count
{
    type Err = Box<dyn Error>;

    fn from_str(text: &str) -> Result<Self, Self::Err>
    {
        Ok(Count(text.len().try_into()?))
    }
}

fn main()
{
}
