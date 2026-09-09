#![allow(dead_code, specification_present)]
#![allow(improper_ctypes)]
#![allow(unused_variables)]

#[repr(transparent)]
struct WrappedU8(u8);

#[repr(transparent)]
struct LocalImpl(u8);

trait LocalTrait
{
    fn must(value: bool) -> char;

    fn provided(
        &self,
        count: i32,
    ) -> u64
    {
        0
    }
}

impl LocalImpl
{
    fn inherent(
        &self,
        flag: bool,
    ) -> u8
    {
        0
    }
}

impl LocalTrait for LocalImpl
{
    fn must(value: bool) -> char
    {
        'x'
    }
}

impl From<u8> for WrappedU8
{
    fn from(value: u8) -> Self
    {
        Self(value)
    }
}

unsafe extern "C" {
    fn foreign(flag: bool) -> u8;
}

fn main()
{
}
