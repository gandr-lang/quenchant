// aux-build: sources.rs
// Sources read from a fixture file, one-line literals, strings built at run
// time and a multi-line text that never reaches the parser's source type.
#![allow(dead_code, unused_variables)]

extern crate sources;

use sources::Other;
use sources::SourceText;
use sources::parse;

fn included()
{
    parse(SourceText::from(include_str!("fixture.gandr")));
}

fn one_line()
{
    parse(SourceText::from("def a = 1"));
    parse(SourceText::from("def b = 2\n"));
}

fn at_run_time(text: &str)
{
    parse(SourceText::from(text));
}

fn an_expected_text_beside_its_source()
{
    for (source, expected) in [("def c = 3", "def c =\n  3\n")] {
        parse(SourceText::from(source));
        let _ = expected.len();
    }
}

fn another_wrapper()
{
    let other = Other::from("def d =\n  4\n");
}

fn a_message()
{
    let message = "first line\nsecond line";
    let _ = message.len();
}

fn main()
{
}
