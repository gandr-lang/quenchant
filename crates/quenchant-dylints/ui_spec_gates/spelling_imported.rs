#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_unqualified))]
#![allow(dead_code, specification_present)]

use anodized::spec;
use anodized::spec as contract;

mod unrelated
{
    pub use quenchant_fixture_macros::client as spec;
}

#[cfg_attr(debug_assertions, spec(requires: !core::mem::needs_drop::<()>()))]
fn imported()
{
}

#[contract]
fn renamed_import()
{
}

#[unrelated::spec]
fn unrelated_macro()
{
}

#[cfg_attr(any(), anodized::spec)]
fn inactive()
{
}

#[cfg_attr(dylint_lib = "quenchant_dylints", allow(spec_attribute_unqualified))]
#[anodized::spec]
fn item_allow()
{
}

#[cfg_attr(dylint_lib = "quenchant_dylints", allow(spec_attribute_unqualified))]
mod permitted
{
    #[anodized::spec]
    fn module_allow()
    {
    }
}

fn main()
{
}
