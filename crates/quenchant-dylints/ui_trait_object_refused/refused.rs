//! A configuration of the classes with a number, an empty entry and a trait of
//! this crate that does not exist: each is reported at the crate root, and the
//! class `refused::Shape` still refuses its objects.

trait Shape
{
    fn area(&self) -> u32;
}

impl Shape for u32
{
    fn area(&self) -> u32
    {
        *self
    }
}

// A trait object of the configured class: refused.

fn measured(shape: &dyn Shape) -> u32
{
    shape.area()
}

fn main()
{
    let _area = measured(&1);
}
