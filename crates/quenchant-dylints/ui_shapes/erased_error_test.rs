// compile-flags: --test
// A `--test` compilation is exempt as a whole: its helpers report failures and
// never match on them.
#![cfg_attr(dylint_lib = "quenchant_dylints", deny(erased_error_signature))]

use core::error::Error;

fn helper() -> Result<(), Box<dyn Error>>
{
    Ok(())
}

#[test]
fn a_test_returns_an_erased_error() -> Result<(), Box<dyn Error>>
{
    helper()
}
