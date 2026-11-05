// aux-build: sources.rs
// A source of more than one line, spelled as a Rust literal, reaches the
// parser's source type.
#![allow(dead_code, unused_variables)]

extern crate sources;

use sources::SourceText;
use sources::parse;
use sources::read;

const SOURCE: &str = "def a =\n  1\n";

fn literal()
{
    parse(SourceText::from("def b =\n  2\n"));
}

fn constant()
{
    parse(SourceText::from(SOURCE));
}

fn constant_again()
{
    parse(SourceText::from(SOURCE));
}

fn helper<'source>(source: impl Into<SourceText<'source>>)
{
    parse(source.into());
}

fn through_a_helper()
{
    helper("def c =\n  3\n");
}

fn through_a_dependency()
{
    read("def d =\n  4\n");
}

fn formatted(name: &str)
{
    let text = format!("def {name} =\n  5\n");
    parse(SourceText::from(text.as_str()));
}

fn concatenated()
{
    parse(SourceText::from(concat!("def f =\n", "  6\n")));
}

fn a_case_table()
{
    let cases = [
        ("def g =\n  7\n", "def g = 7\nas laid out"),
        ("def h = 8", "def h = 8\n"),
    ];
    for (source, expected) in cases {
        parse(SourceText::from(source));
        let _ = expected.len();
    }
}

fn a_closure()
{
    let reader = |text: &str| parse(SourceText::from(text));
    reader("def i =\n  9\n");
}

fn pushed()
{
    let mut text = String::new();
    text.push_str("def j =\n  10\n");
    parse(SourceText::from(text.as_str()));
}

fn mapped()
{
    let parsed: Vec<usize> = ["def k =\n  11\n"]
        .iter()
        .map(|text| parse(SourceText::from(*text)))
        .collect();
}

fn included_from_another_extension()
{
    parse(SourceText::from(include_str!("fixture.txt")));
}

fn main()
{
}
