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

With the facade feature enabled, `anodized::types::Spec` exposes the fork's refinement trait. Generated refinement implementations exist only under an instrumentation cfg, so tests calling `predicate` select that mode. Strip mode supplies no duplicate refinement implementation.

## Build modes

| Configuration | What the consumer receives | What it establishes |
| ------------- | -------------------------- | ------------------- |
| No instrumentation cfg, with or without the backend feature | Ordinary code with supported specification markers removed | No executable-check evidence |
| `anodized_panic` with the facade feature | Panic-enforcing checks, compatible with `no_std` | Evidence for the interpreted predicates on calls that reach them |
| `anodized_print` with the facade feature | Printed violations; requires `std` and rejects specified `const fn` | Diagnostics without rejection of invalid calls |

Both strip mode and panic enforcement support targets without `std`. Procedural macros use the build host's standard library, which adds no target runtime dependency. Required validation and safety checks remain ordinary code, independent of this feature.

The build driver selects native enforcement with `RUSTFLAGS="--cfg anodized_panic"` and `--features quenchant-anodized/anodized`. The cfg applies throughout the dependency graph; no per-consumer feature can leave part of that graph stripped. A cfg without the facade feature fails compilation with `ANODIZED_BACKEND_DISABLED`. Feature unification and `--all-features` alone never instrument a consumer.

The repository tasks select the mode and run deliberate violations. Cross-target flags need not configure host procedural macros, and a target cfg listing cannot certify a cached host artifact.

## Nested syntax and expansion boundary

Import `anodized::spec` and apply `#[spec(...)]` to the enclosing trait or implementation; its nested methods use bare `#[spec(...)]` markers. Empty nested markers use `#[spec()]`. Qualified nested markers are not an additional interface.

When disabled, the wrapper removes nested markers as well as the outer annotation. Unrelated attributes and macro-language payloads retain their tokens. Enabled expansions resolve `::anodized::__`, `result`, and `types` through re-exports of the fork's runtime, with its default features disabled. No runtime implementation is copied into the facade.

## Specification, evidence, and future interpretation

A specification describes admitted behavior; satisfaction relates an implementation to that description. Adding a requirement refines the specification. Checking or proving an existing requirement strengthens evidence instead. Adequacy asks whether the statements, observations, and evidence distinguish relevant deviations—not merely whether named tests pass.

Omitting executable checking changes neither the authored obligation nor its authority. It also supplies no evidence for that obligation. A future verification adapter must consume authored source or a deliberately preserved representation and relate it to the selected build. Removed clauses are not presumed to remain in stripped HIR, and this facade establishes no cross-verifier correspondence or full-abstraction theorem.

The backend rows pair `version = "=0.7.0"` with an exact Git revision. Cargo uses the fork locally and removes the Git source when packaging; the fork's package version must therefore equal a published upstream version. Upstream 0.7.0 lacks the required `no_std`, const-specification, and logic-free refinement combination. Replace the Git rows when an upstream release carries those capabilities, or when the fork is published under organization-owned package names.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
