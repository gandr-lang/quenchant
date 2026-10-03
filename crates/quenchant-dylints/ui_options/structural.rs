type Missing<T> = Option<T>;
type Nested = Result<Missing<Value>, Value>;

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Value(u8);

fn direct(value: Option<Value>) -> Missing<Value> { value }
fn nested(value: &[(Nested, *const Missing<Value>)]) { let _ = value; }
fn callback(value: fn(Value) -> Option<Value>) { let _ = value; }
async fn asynchronous() -> Missing<Value> { None }
fn future() -> impl core::future::Future<Output = Missing<Value>> { async { None } }
fn dynamic(value: &dyn Iterator<Item = Missing<Value>>) { let _ = value; }
fn borrowed<'a>(value: &'a Value) -> impl core::future::Future<Output = Missing<&'a Value>> + 'a { async move { Some(value) } }
fn borrowed_clean<'a>(value: &'a [Value]) -> impl Iterator<Item = &'a Value> + 'a { value.iter() }

trait Local {
    fn required(value: Missing<Value>);
    fn provided() -> Option<Value> { None }
}
impl Local for Value {
    fn required(value: Missing<Value>) { let _ = value; }
}
impl Value {
    const fn inherent(self) -> Option<Self> { Some(self) }
}

#[allow(unsafe_safety_documentation)] unsafe extern "C" {
    fn foreign(value: Option<core::num::NonZeroU8>) -> Option<core::num::NonZeroU8>;
}

fn main() {}
