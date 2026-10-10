#![crate_name = "quenchant_shape"]
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(maybe_shape))]

// This fixture must exercise the producer without its diagnostic-item marker.
#[cfg(quenchant_compiler_policy)]
compile_error!("the producer identity witness requires an unmarked definition");

#[path = "../../quenchant-shape/src/shape.rs"]
mod shape;

// Sharing the producer's crate and item name does not establish its identity.
enum Maybe<Value, Reason>
{
    Present(Value),
    Absent(Reason),
}

// The canonical module must belong directly to the crate root.
mod nested
{
    pub mod shape
    {
        pub enum Maybe<Value, Reason>
        {
            Present(Value),
            Absent(Reason),
        }
    }
}

fn main()
{
}
