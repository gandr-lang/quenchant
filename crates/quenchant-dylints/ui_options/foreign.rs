#[repr(transparent)]
struct Value(u8);

#[repr(transparent)]
struct Sequence(Value);
impl Iterator for Sequence {
    type Item = Value;
    fn next(&mut self) -> Option<Self::Item> { None }
}

#[repr(transparent)]
struct NestedSequence(Value);
impl Iterator for NestedSequence {
    type Item = Option<Value>;
    fn next(&mut self) -> Option<Self::Item> { None }
}

impl From<Option<Value>> for Value {
    fn from(value: Option<Value>) -> Self {
        match value { Some(value) => value, None => Self(0) }
    }
}

// A transparent wrapper is a signature boundary; its own field still answers
// to `option_field`, which this fixture does not demonstrate.
#[allow(option_field)]
#[repr(transparent)]
struct Hidden(Option<Value>);
fn nominal(value: Hidden) -> Hidden { value }

fn main() {}
