// A parser's source type and a helper that builds it from a string, as a
// dependency of the fixtures holds them.
#![allow(dead_code)]

/// The text a parser reads.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct SourceText<'source>(&'source str);

impl<'source> From<&'source str> for SourceText<'source>
{
    fn from(text: &'source str) -> Self
    {
        Self(text)
    }
}

/// A wrapper of the same shape that no configuration names.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Other<'source>(&'source str);

impl<'source> From<&'source str> for Other<'source>
{
    fn from(text: &'source str) -> Self
    {
        Self(text)
    }
}

/// Parse a source.
pub fn parse(source: SourceText<'_>) -> usize
{
    let _ = source;
    0
}

/// Parse a source handed as a string.
pub fn read(text: &str) -> usize
{
    parse(SourceText::from(text))
}
