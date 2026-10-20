// Refused: a buffer's front removed or inserted on every pass of a loop over
// a buffer bound outside it, one function per operation and repetition.
#![allow(dead_code, unused_variables)]

// Refused: `Vec::remove(0)` draining a queue in a `while` body.
fn remove_front_in_while(mut queue: Vec<u32>)
{
    while !queue.is_empty() {
        let next = queue.remove(0);
    }
}

// Refused: `Vec::insert(0, _)` prepending in a `for` body.
fn insert_front_in_for(values: &[u32]) -> Vec<u32>
{
    let mut reversed = Vec::new();
    for value in values {
        reversed.insert(0, *value);
    }
    reversed
}

// Refused: a front-anchored `drain` in a `loop` body.
fn drain_prefix_in_loop(
    buffer: &mut Vec<u8>,
    chunk: usize,
)
{
    loop {
        if buffer.len() < chunk {
            break;
        }
        let taken: Vec<u8> = buffer.drain(.. chunk).collect();
    }
}

// Refused (#66): `String::insert_str(0, _)` and `String::insert(0, _)` in a
// `for` body.
fn prepend_text(parts: &[&str]) -> String
{
    let mut text = String::new();
    for part in parts {
        text.insert_str(0, part);
        text.insert(0, '/');
    }
    text
}

// Refused (#66): `String::remove(0)` in a `while` body.
fn strip_leading(mut text: String)
{
    while text.starts_with(' ') {
        text.remove(0);
    }
}

// Refused: `Vec::insert(0, _)` in a closure `for_each` calls once per element.
fn insert_front_in_for_each(values: &[u32]) -> Vec<u32>
{
    let mut reversed = Vec::new();
    values.iter().for_each(|value| reversed.insert(0, *value));
    reversed
}

fn main()
{
}
