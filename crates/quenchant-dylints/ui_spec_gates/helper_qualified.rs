#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_unqualified))]
#![allow(dead_code, specification_present, unused_imports)]

use anodized as facade;

mod exported
{
    pub use anodized::spec_helper as helper;
}

#[anodized::spec_helper]
fn direct()
{
}

#[::anodized::spec_helper]
struct Absolute;

#[facade::spec_helper]
use core::cmp::Ordering;

#[exported::helper]
fn reexport()
{
}

#[cfg_attr(debug_assertions, anodized::spec_helper)]
fn conditional()
{
}

#[cfg_attr(debug_assertions, cfg_attr(debug_assertions, anodized::spec_helper))]
fn nested_conditional()
{
}

impl Absolute
{
    #[anodized::spec_helper]
    fn method(&self)
    {
    }
}

fn main()
{
}
