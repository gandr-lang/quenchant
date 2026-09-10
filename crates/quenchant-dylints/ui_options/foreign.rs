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

#[repr(transparent)]
struct Hidden(Option<Value>);
fn nominal(value: Hidden) -> Hidden { value }

fn main() {}
