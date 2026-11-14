//! Outside the default, an empty base record is a unit new at the base. Each
//! of its functions is held.

fn kept(values: &mut Vec<u32>)
{
    values.push(1);
}

fn changed(values: &mut Vec<u32>)
{
    values.push(2);
}

fn fresh(values: &mut Vec<u32>)
{
    values.push(3);
}

fn main()
{
    let mut values = Vec::new();
    kept(&mut values);
    changed(&mut values);
    fresh(&mut values);
}
