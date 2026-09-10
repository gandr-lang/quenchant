// compile-flags: -Aoption_signature
// This dependency defines the foreign signature; only its consumer opts in.
pub trait Boundary<T> {
    fn required(value: Option<T>) -> Option<T>;
    fn generic(value: T) -> T;
    fn callback(value: fn(Option<T>) -> Option<T>);
    fn dynamic(value: &dyn Iterator<Item = Option<T>>);
    fn asynchronous() -> impl core::future::Future<Output = Option<T>>;
    fn future() -> impl core::future::Future<Output = Option<T>>;
    fn argument(value: &dyn Carrier<Option<T>>);
    fn opaque() -> impl Carrier<Option<T>>;
}

pub trait Carrier<T> {}
impl<T> Carrier<T> for () {}
