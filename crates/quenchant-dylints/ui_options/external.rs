// aux-build: foreign_policy.rs
extern crate foreign_policy;

#[repr(transparent)]
struct Value(u8);

impl foreign_policy::Boundary<Value> for Value {
    fn required(value: Option<Value>) -> Option<Value> { value }
    fn generic(value: Value) -> Value { value }
    fn callback(value: fn(Option<Value>) -> Option<Value>) { let _ = value; }
    fn dynamic(value: &dyn Iterator<Item = Option<Value>>) { let _ = value; }
    async fn asynchronous() -> Option<Value> { None }
    fn future() -> impl core::future::Future<Output = Option<Value>> { async { None } }
    fn argument(value: &dyn foreign_policy::Carrier<Option<Value>>) { let _ = value; }
    fn opaque() -> impl foreign_policy::Carrier<Option<Value>> { () }
}

#[repr(transparent)]
struct Introduced(Value);
impl foreign_policy::Boundary<Option<Value>> for Introduced {
    fn required(value: Option<Option<Value>>) -> Option<Option<Value>> { value }
    fn generic(value: Option<Value>) -> Option<Value> { value }
    fn callback(value: fn(Option<Option<Value>>) -> Option<Option<Value>>) { let _ = value; }
    fn dynamic(value: &dyn Iterator<Item = Option<Option<Value>>>) { let _ = value; }
    async fn asynchronous() -> Option<Option<Value>> { None }
    fn future() -> impl core::future::Future<Output = Option<Option<Value>>> { async { None } }
    fn argument(value: &dyn foreign_policy::Carrier<Option<Option<Value>>>) { let _ = value; }
    fn opaque() -> impl foreign_policy::Carrier<Option<Option<Value>>> { () }
}

fn local_dynamic(value: &dyn foreign_policy::Carrier<Option<Value>>) { let _ = value; }
fn local_opaque() -> impl foreign_policy::Carrier<Option<Value>> { () }

fn main() {}
