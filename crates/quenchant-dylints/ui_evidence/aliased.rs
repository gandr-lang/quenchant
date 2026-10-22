//! A prose clause linking the standard library's `Ok` and `Err` from a
//! function returning a local alias of `Result`, beside the variants of a
//! local enum named alike. rustdoc's JSON lists the linked variants under the
//! alias's path, in this crate, with the standard library's crate id.
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

/// What reading a spelling comes to.
pub type Answer = Result<Daily, ()>;

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
pub fn lane(text: &str) -> Answer
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
