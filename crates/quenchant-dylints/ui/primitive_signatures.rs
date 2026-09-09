// edition:2024
#![allow(dead_code, specification_present)]
#![allow(improper_ctypes)]
#![allow(unused_variables)]

use std::borrow::Cow;
use std::cell::Cell;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

#[repr(transparent)]
struct WrappedU8(u8);

#[repr(transparent)]
struct WrappedBool(bool);

struct LocalContainer<T>
{
    value: T,
    tag: WrappedU8,
}

#[repr(transparent)]
struct TransparentGeneric<T>(T);

type AliasU8 = u8;
type AliasTuple = (i32, WrappedU8);
type GenericAlias<T> = Option<T>;
type ExternalAlias = std::os::raw::c_int;

fn direct_primitives(
    bool_value: bool,
    char_value: char,
    f32_value: f32,
    f64_value: f64,
    i8_value: i8,
    i16_value: i16,
    i32_value: i32,
    i64_value: i64,
    i128_value: i128,
    u8_value: u8,
    u16_value: u16,
    u32_value: u32,
    u64_value: u64,
    u128_value: u128,
    isize_value: isize,
    usize_value: usize,
    str_value: &str,
) -> bool
{
    bool_value
}

async fn direct_async(value: u8) -> bool
{
    value == 0
}

fn structural_primitives(
    ref_bool: &bool,
    raw_char: *const char,
    array_f32: [f32; 1],
    slice_f64: &[f64],
    tuple_indices: (isize, usize),
    callback: fn(u8) -> i16,
    alias_u8: AliasU8,
    alias_tuple: AliasTuple,
) -> *mut str
{
    loop {}
}

fn transparent_containers(
    option_bool: Option<bool>,
    result_usize: Result<usize, WrappedU8>,
    vec_char: Vec<char>,
    boxed_u8: Box<u8>,
    rc_i32: Rc<i32>,
    arc_u64: Arc<u64>,
    cow_str: Cow<'static, str>,
    cell_bool: Cell<bool>,
    refcell_f32: RefCell<f32>,
)
{
}

fn general_generic_containers(
    range: Range<u64>,
    local: LocalContainer<u64>,
)
{
}

fn alias_edges(
    generic_alias: GenericAlias<u32>,
    external_alias: ExternalAlias,
)
{
}

fn generic_alias_nominal_leaf_is_accepted(value: GenericAlias<WrappedU8>)
-> GenericAlias<WrappedU8>
{
    value
}

fn nominal_container_leaf_is_accepted(option: Option<WrappedBool>) -> Option<WrappedBool>
{
    option
}

fn transparent_generic_is_accepted(input: TransparentGeneric<u64>) -> TransparentGeneric<u64>
{
    input
}

fn nominal_wrapper_is_accepted(
    input: WrappedU8,
    output: &WrappedBool,
) -> WrappedU8
{
    input
}

async fn async_nominal_wrapper_is_accepted(value: WrappedU8) -> WrappedBool
{
    let _ = value;
    WrappedBool(true)
}

async fn async_nested_primitive(value: WrappedU8) -> Option<u8>
{
    let _ = value;
    None
}

async fn async_nested_primitive_under_layers(value: WrappedU8) -> Result<Vec<(u8, char)>, WrappedU8>
{
    let _ = value;
    Err(WrappedU8(0))
}

async fn async_nested_wrapper_is_accepted(value: WrappedU8) -> Option<WrappedBool>
{
    let _ = value;
    None
}

fn unit_is_accepted(value: ()) -> ()
{
    value
}

fn never_is_accepted() -> !
{
    loop {}
}

fn main()
{
}
