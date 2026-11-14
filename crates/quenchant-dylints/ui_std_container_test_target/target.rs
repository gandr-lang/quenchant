//! A listed crate built as a test target: a container in a test is the test's
//! scratch, and nothing is refused.

fn scratch() -> Vec<u8>
{
    vec![1_u8, 2]
}

#[test]
fn reads_its_scratch()
{
    let text = format!("{}", 1_u8);
    assert_eq!(scratch().len(), 2);
    assert_eq!(text.len(), 1);
}
