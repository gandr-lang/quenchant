// compile-flags: --test
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(spec_attribute_unqualified))]
#![allow(dead_code, specification_present)]

use anodized::spec;

#[spec]
fn subject()
{
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    #[spec]
    fn inherited_import()
    {
        subject();
    }
}
