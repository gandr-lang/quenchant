//! A prose clause linking the standard library's `Ok` and `Err` beside
//! workspace items named alike: the associated type of a `FromStr` impl and
//! the variants of a local enum. rustdoc's JSON lists the linked variants
//! under no path.
#![allow(dead_code, primitive_signature)]

use core::str::FromStr;

/// The lane a schedule names.
pub struct Daily;

impl FromStr for Daily
{
    /// Why a spelling names no lane.
    type Err = ();

    /// The lane `daily` spells.
    ///
    /// # Specification
    /// trivial.
    fn from_str(text: &str) -> Result<Self, Self::Err>
    {
        if text == "daily" { Ok(Self) } else { Err(()) }
    }
}

/// A spelling's verdict, as a log line names it.
pub enum Logged
{
    /// Read.
    Ok,
    /// Refused.
    Err,
}

/// Read a schedule's lane.
///
/// # Specification
/// - ensures: [`Ok`] for `daily`; [`Err`] for any other spelling.
pub fn lane(text: &str) -> Result<Daily, ()>
{
    text.parse()
}

/// The fixture compiles as a program.
///
/// # Specification
/// trivial.
fn main()
{
}
