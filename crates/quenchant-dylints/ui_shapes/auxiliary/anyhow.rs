// A stand-in for the dependency of this name: the lint resolves its error type
// by path, so only the path and the type's identity matter here.
pub struct Error;

pub type Result<Value, Failure = Error> = core::result::Result<Value, Failure>;
