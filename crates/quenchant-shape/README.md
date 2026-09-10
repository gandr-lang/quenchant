# quenchant-shape

Keep non-failure absence and domain identity explicit. `Maybe<Value, Reason>` separates a present value from the reason it is absent. Supporting macros declare closed reason sites, transparent nominal types, and selected operator implementations without imposing an arithmetic policy.

## Install

From an application beside a workspace checkout:

```toml
[dependencies]
quenchant-shape = { version = "=0.0.0", path = "../quenchant/crates/quenchant-shape" }
```

Registry-only installation follows publication. The default library is `no_std`, uses no allocator, and forbids unsafe code in its implementation.

Compiler-policy builds use the internal `quenchant_compiler_policy` cfg to register `Maybe`'s diagnostic identity for the matched nightly plugin. This is a whole-graph compiler flag, not a Cargo feature; ordinary and `--all-features` library builds retain the declared stable compiler boundary. The [plugin's activation guide](../quenchant-dylints/README.md#absence-signatures) specifies flag composition, external dependencies, and explicit lint enablement.

## Example

```rust
use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    pub mod lookup {
        #[derive(Debug, Eq, PartialEq)]
        pub enum Unavailable { NotFound, NotSearched }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Label { Original, Changed }

let present: Maybe<Label, lookup::Unavailable> = Maybe::Present(Label::Original);
assert_eq!(present.map(|_| Label::Changed), Maybe::Present(Label::Changed));

let absent: Maybe<Label, lookup::Unavailable> = Maybe::Absent(lookup::Unavailable::NotSearched);
assert_eq!(
    absent.map(|_| Label::Changed),
    Maybe::Absent(lookup::Unavailable::NotSearched),
);
```

`Label` is not `Copy` or `Clone`. The combinator consumes its payload once; it does not need to duplicate the value or absence evidence.

## Absence is not failure

- `map` invokes its `FnOnce` mapper only for `Present`; an absence keeps its exact reason.
- `and_then` allows a present value to become a value or an absence in the same reason domain. An existing absence bypasses the continuation.
- `absent_reason` borrows a reason. A present value produces the query site's explicit `ValuePresent` reason rather than an uninformative empty result.
- `into_result` promotes absence to failure through an explicit mapper whose error implements `core::error::Error`.

There is no `Try` implementation, implicit conversion to an error channel, or invented default reason. The container does not decide whether a caller's reason type has the correct domain meaning. `reason_enum!` seals a site-specific reason trait to the declared enum; a local API can name that enum or use that site's trait bound.

## Nominal scaffolding

`nominal_type!` declares a one-field transparent type with a private representation. It adds no constructor, `Deref`, conversion, or default beyond the attributes supplied by the caller. Validation belongs to the owning domain.

`delegate_ops!` has binary, unary, and assignment forms. It invokes the selected safe operation once at the standard-trait boundary and rewraps value results. It neither chooses arithmetic semantics nor introduces cloning or an unsafe block. Safe arithmetic operators should use a permanently safe operation, not a feature-dependent unchecked entry point.

Generated operator methods refer to the `quenchant` specification facade. A crate using that macro supplies the dependency name and feature mapping described in [the facade guide](../quenchant-anodized/README.md).

## Specifications and the extraction example

The optional `anodized` feature enables `std`-based instrumentation. Combinator postconditions borrow move-only results through `|ref output|`; they observe variant preservation without replaying a callback or adding equality bounds. Exact payload and callback behavior have separate executable witnesses.

`examples/verus_derive.rs` reads authored source and translates its supported predicates into verifier input. Run from `crates/quenchant-shape/` so its source-relative paths select the intended package:

```sh
mise exec -- cargo run --example verus_derive
```

`VERUS_BIN` must name a verifier executable. Its absence is an operational failure, not a successful run over zero obligations. The example names unreached source separately and reports verifier refusals without crediting them as accepted obligations.

Source extraction, verifier output, and correspondence to production are different claims. The example does not establish a general proof-transfer theorem.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
