extern crate quenchant_shape as canonical;
extern crate foreign_shape as foreign;

pub use canonical::shape::Maybe as Reexported;
type CanonicalAlias<T, R> = Reexported<T, R>;
pub use foreign::shape::Maybe as ForeignReexport;
type ForeignAlias<T, R> = ForeignReexport<T, R>;

fn canonical(_: canonical::shape::Maybe<Option<u8>, ()>) {}
fn canonical_alias(_: CanonicalAlias<Option<u8>, ()>) {}
fn canonical_reexport(_: Reexported<Option<u8>, ()>) {}
fn foreign(_: foreign::shape::Maybe<Option<u8>, ()>) {}
fn foreign_alias(_: ForeignAlias<Option<u8>, ()>) {}
fn foreign_reexport(_: ForeignReexport<Option<u8>, ()>) {}
fn unmarked(_: unmarked_shape::shape::Maybe<Option<u8>, ()>) {}
fn direct_option(_: Option<u8>) {}

fn main() {}
