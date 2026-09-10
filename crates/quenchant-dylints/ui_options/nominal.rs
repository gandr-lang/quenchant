#[repr(transparent)]
struct Value(u8);

enum Option<T> { Present(T), Empty }

fn nominal_spelling(value: Option<Value>) -> Option<Value> { value }

#[repr(transparent)]
struct Hidden(std::option::Option<Value>);
fn nominal_boundary(value: Hidden) -> Hidden { value }

quenchant_shape::reason_enum! {
    mod lookup { pub enum Missing { NotStored } }
}
fn reason_bearing() -> quenchant_shape::shape::Maybe<Value, lookup::Missing> {
    quenchant_shape::shape::Maybe::Absent(lookup::Missing::NotStored)
}

fn main() {}
