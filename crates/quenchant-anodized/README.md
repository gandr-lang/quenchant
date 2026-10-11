# quenchant-anodized

The package provides the `anodized` library and its `spec` attribute, separating an authored obligation from executable checking. The facade is `no_std`. Its optional backend is [gandr-lang/anodized](https://github.com/gandr-lang/anodized), pinned for `no_std` enforcement, `const fn` specifications, and type refinements without the logic layer.

## Install

Add the package by its own name; Rust imports its library as `anodized`. From an application beside a checkout:

```toml
[dependencies]
quenchant-anodized = { version = "=0.0.0", path = "../quenchant/crates/quenchant-anodized", default-features = false }

```

A registry release permits removing the path. No consumer feature is required. Declare `anodized_panic` and `anodized_print` in the consumer's `check-cfg` list when denying unexpected cfgs.

The facade's `anodized` feature compiles the backend in for development and CI; enabling it from a registry release selects upstream packages and is unsupported.

## Example

```rust
use anodized::spec;

#[spec(ensures: |ref output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}

assert_eq!(accept(), Ok(()));
```

Postcondition patterns such as `|ref output|` borrow the returned value for inspection without adding `Copy` or `Clone` bounds. The backend also supports owned patterns that it can reconstruct after checking.

With the facade feature enabled, `anodized::types::Spec` exposes the fork's refinement trait. Generated refinement implementations exist in both plain and enforcing backend builds. Without the feature, refinement annotations are erased.

## Build modes

| Surface | Backend feature off | Backend on, no cfg | Backend on, `anodized_panic` | Backend on, `anodized_print` |
| ------- | ------------------- | ------------------ | ---------------------------- | ---------------------------- |
| `#[spec]` | Erases supported markers; no backend dependency | Type-checks predicates without evaluating them; evaluates captures | Enforces predicates by panicking | Prints violations; requires `std` and rejects specified `const fn` |
| `#[spec_helper]` | Erases the annotated item or binding, including its initializer | Retains the original tokens | Retains the original tokens | Retains the original tokens |
| Evidence | No executable-check evidence | No executable-check evidence | Interpreted predicates on calls that reach them | Diagnostics without rejecting invalid calls |

Erasure, backend-on plain mode, and panic enforcement support targets without `std`. Procedural macros use the build host's standard library, which adds no target runtime dependency. Required validation and safety checks remain ordinary code, independent of this feature.

The build driver selects native enforcement with `RUSTFLAGS="--cfg anodized_panic"` and `--features quenchant-anodized/anodized`. The cfg applies throughout the dependency graph; no per-consumer feature can leave part of that graph stripped. Set the same cfg in `RUSTDOCFLAGS` when running rustdoc. A cfg without the facade feature fails compilation with `ANODIZED_BACKEND_DISABLED`. Feature unification and `--all-features` alone never enable violation checking.

The repository tasks select the mode and run deliberate violations. Cross-target flags need not configure host procedural macros, and a target cfg listing cannot certify a cached host artifact.

Backend-on plain mode retains predicate-only helpers, imports, and bindings by reusing the fork emitter instead of erasing clauses or duplicating its parser. Captures retain the fork's evaluation semantics. Revisit that choice if the backend offers a separate compile-only emitter with equivalent reference and capture behavior.

Feature-off erasure is intended for downstream dependency builds, where rustc caps dependency lints.

## Specification-only helpers

Import `anodized::spec_helper` beside `spec`. Annotate helpers and imports used only by predicates so feature-off builds also compile under `-D warnings`:

```rust
use anodized::{spec, spec_helper};

#[spec_helper]
#[derive(PartialEq)]
enum ConstEquality { Equal, Different }

#[spec_helper]
const fn const_eq(left: u8, right: u8) -> ConstEquality {
    if left == right { ConstEquality::Equal } else { ConstEquality::Different }
}

#[spec(ensures: |output| const_eq(output, value) == ConstEquality::Equal)]
fn preserve(value: u8) -> u8 { value }

assert_eq!(preserve(7), 7);
```

`spec_helper` names an auxiliary to `spec`, rather than a runtime check or a build selector. The facade selects its erasing or retaining export from its own backend feature; consumer cfgs and feature names are unnecessary. A consumer-side `cfg` would duplicate that selection, and allowing unused items would retain unnecessary code. Revisit the separate attribute if the backend gains a specification-only declaration form.

The attribute accepts no arguments. It supports items such as functions, types, imports, and local constants. On a local `let` statement, Rust additionally requires nightly `#![feature(proc_macro_hygiene)]`. In retained mode, initializers keep their ordinary evaluation semantics; in strip mode they disappear. Helpers must not perform required validation or other necessary effects.

Function parameters are not attribute-macro invocation sites. A predicate-only parameter in a trait implementation keeps its signature and uses the backend-on route: predicates retain the parameter reference in plain and enforcing builds. `spec_helper` does not remove parameters or suppress warnings on the enclosing function. Nested loop predicates are outside the backend's executable-check support; annotating a local binding does not supply that missing interpretation.

## Nested syntax and expansion boundary

Import `anodized::spec` and apply `#[spec(...)]` to the enclosing trait or implementation; its nested methods use bare `#[spec(...)]` markers. Empty nested markers use `#[spec()]`. Qualified nested markers are not an additional interface.

With the backend feature off, the wrapper removes nested markers as well as the outer annotation. Unrelated attributes and macro-language payloads retain their tokens. With the feature on, the facade exports the fork's attribute in every cfg mode; expansions resolve `::anodized::__`, `result`, and `types` through re-exports of the fork's runtime, with its default features disabled. No parser or runtime implementation is copied into the facade.

## Specification, evidence, and future interpretation

A specification describes admitted behavior; satisfaction relates an implementation to that description. Adding a requirement refines the specification. Checking or proving an existing requirement strengthens evidence instead. Adequacy asks whether the statements, observations, and evidence distinguish relevant deviations—not merely whether named tests pass.

Omitting executable checking changes neither the authored obligation nor its authority. It also supplies no evidence for that obligation. A future verification adapter must consume authored source or a deliberately preserved representation and relate it to the selected build. Removed clauses are not presumed to remain in stripped HIR, and this facade establishes no cross-verifier correspondence or full-abstraction theorem.

The backend rows pair `version = "=0.7.0"` with an exact Git revision. Cargo uses the fork locally and removes the Git source when packaging; the fork's package version must therefore equal a published upstream version. Upstream 0.7.0 lacks the required `no_std`, const-specification, and logic-free refinement combination. Replace the Git rows when an upstream release carries those capabilities, or when the fork is published under organization-owned package names.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
