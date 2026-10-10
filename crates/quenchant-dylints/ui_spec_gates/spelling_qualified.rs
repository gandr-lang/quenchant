#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_unqualified))]
#![allow(dead_code, specification_present)]

use anodized as contracts;

mod exported
{
    pub use anodized::spec as contract;
}

#[anodized::spec(requires: !core::mem::needs_drop::<Subject>())]
fn direct()
{
}

#[::anodized::spec]
fn absolute()
{
}

#[contracts::spec]
fn crate_alias()
{
}

#[exported::contract]
fn reexport()
{
}

#[anodized_backend::spec]
fn backend()
{
}

#[cfg_attr(debug_assertions, anodized::spec(requires: !core::mem::needs_drop::<Subject>()))]
fn conditional()
{
}

#[cfg_attr(debug_assertions, cfg_attr(debug_assertions, anodized::spec))]
fn nested_conditional()
{
}

struct Subject;

impl Subject
{
    #[anodized::spec]
    fn method(&self)
    {
    }
}

#[anodized::spec]
trait Contract
{
    fn operation(&self);
}

fn main()
{
}
