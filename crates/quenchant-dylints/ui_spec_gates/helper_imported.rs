#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_unqualified))]
#![allow(dead_code, specification_present, unused_imports)]

use anodized::spec_helper;
use anodized::spec_helper as helper;

mod unrelated
{
    pub use quenchant_fixture_macros::client as spec_helper;
}

#[spec_helper]
use core::cmp::Ordering;

#[cfg_attr(debug_assertions, spec_helper)]
fn imported()
{
}

#[helper]
fn renamed_import()
{
}

#[unrelated::spec_helper]
fn unrelated_macro()
{
}

#[cfg_attr(any(), anodized::spec_helper)]
fn inactive()
{
}

#[cfg_attr(dylint_lib = "quenchant_dylints", allow(spec_attribute_unqualified))]
#[anodized::spec_helper]
fn item_allow()
{
}

#[cfg_attr(dylint_lib = "quenchant_dylints", allow(spec_attribute_unqualified))]
mod permitted
{
    #[anodized::spec_helper]
    fn module_allow()
    {
    }
}

fn main()
{
}
