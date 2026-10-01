// run-rustfix
// Taking `&raw` through a raw pointer needs no `unsafe`, so the fixed form
// leaves those blocks unused.
#![allow(dead_code, unused_variables, unused_unsafe)]

use core::ptr;

struct Frame
{
    header: u32,
    words: [u32; 4],
}

fn takes_mut(_: *mut u32)
{
}

fn takes_slice(_: *const [u32])
{
}

// Refused: a borrow coerced through an unsizing step.
fn unsizing_borrow()
{
    let words = [0_u32; 4];
    let pointer: *const [u32] = &words;
    takes_slice(&words);
}

// Refused: a reference value coerced to a raw pointer.
fn reference_value(
    value: &mut u32,
    shared: &u32,
)
{
    let mutable: *mut u32 = value;
    let constant: *const u32 = shared;
    let narrowed: *const u32 = value;
    takes_mut(value);
}

// Refused: a slice pointer method that autorefs an owned place.
fn owned_place()
{
    let mut words = [0_u32; 4];
    let first = words.as_mut_ptr();
    let read = words.as_ptr();
    let mut boxed: Box<[u32]> = Box::new([0; 4]);
    let boxed_first = boxed.as_mut_ptr();
    // The fix must address the allocation, `*boxed`, not the box itself.
    let explicit_boxed = (&mut boxed).as_mut_ptr();
    let mut frame = Frame {
        header: 0,
        words: [0; 4],
    };
    let field = frame.words.as_mut_ptr();
}

// Refused: a reference created through a raw pointer before the pointer method.
fn through_raw_pointer(
    raw: *mut [u32; 4],
    frame: *mut Frame,
)
{
    let explicit = unsafe { (&mut *raw).as_mut_ptr() };
    let implicit = unsafe { (*raw).as_mut_ptr() };
    let field = unsafe { (*frame).words.as_ptr() };
}

// Refused: a fresh borrow handed to a pointer constructor.
fn constructors()
{
    let mut word = 0_u32;
    let mutable = ptr::from_mut(&mut word);
    let constant = core::ptr::from_ref(&word);
    // The fix must keep the method applied to the pointer, not to `word`.
    let chained = core::ptr::from_mut(&mut word).cast_const();
    // The fix must keep the slice pointer the type parameter selected.
    let words = [0_u32; 2];
    let unsized_pointer = core::ptr::from_ref::<[u32]>(&words);
    let mut buffer = [0_u32; 2];
    let unsized_length = ptr::from_mut::<[u32]>(&mut buffer).len();
}

// Accepted: receivers and arguments that are already reference values.
fn accepted(
    slice: &[u32],
    words: &mut [u32; 4],
    frame: &Frame,
    value: &u32,
)
{
    let read = slice.as_ptr();
    let first = words.as_mut_ptr();
    let reborrowed = (&*slice).as_ptr();
    let field = frame.words.as_ptr();
    let constant = ptr::from_ref(value);
    // An overloaded dereference creates a reference `&raw` cannot avoid.
    let shared: std::rc::Rc<[u32]> = std::rc::Rc::from([0_u32; 2]);
    let through_rc = (&shared).as_ptr();
    let mut vector = vec![0_u32; 4];
    let element = vector.as_mut_ptr();
    let indexed = vector[0 .. 2].as_ptr();
}

// Accepted: the raw forms the suggestions produce.
fn raw_forms(raw: *mut [u32; 4])
{
    let mut words = [0_u32; 4];
    let first = (&raw mut words).cast::<u32>();
    let through = raw.cast::<u32>();
    let whole: *const [u32] = &raw const words;
}

// Left to Clippy's `borrow_as_ptr`: the plain implicit borrow.
fn plain_borrow()
{
    let mut word = 0_u32;
    let pointer: *mut u32 = &mut word;
}

fn main()
{
}
