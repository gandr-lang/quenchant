# quenchant-anodized

The public `#[quenchant::spec(...)]` interface separates an authored obligation from the choice to emit executable checking. The facade is `no_std`. Its optional backend is the published `anodized-macros` package, not a copied runtime or logic implementation.

## Install

Use the dependency name `quenchant`, because emitted helper paths use that name. From an application beside a checkout:

```toml
[dependencies]
quenchant = { package = "quenchant-anodized", version = "=0.0.0-rc.0", path = "../quenchant/crates/quenchant-anodized" }

[features]
anodized = ["quenchant/anodized"]
```

A registry release permits removing the path. The feature must be declared in the consuming crate; enabling a dependency feature is not a substitute for the consumer-side condition.

## Example

```rust
# extern crate quenchant_anodized as quenchant;
#[quenchant::spec(ensures: |ref output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}

assert_eq!(accept(), Ok(()));
```

Postcondition patterns such as `|ref output|` borrow the returned value for inspection. The published backend otherwise moves the result into its predicate binding; borrowing permits move-only values without adding `Copy` or `Clone` bounds.

## Three separate choices

| Configuration                                      | What the consumer receives                                 | What it establishes                                    |
| -------------------------------------------------- | ---------------------------------------------------------- | ------------------------------------------------------ |
| Consumer feature absent                            | Ordinary code with supported specification markers removed | No executable-check evidence                           |
| Consumer feature present, no enforcing backend cfg | Published backend expansion, requiring `std`               | Predicate compilation, not violation panics            |
| Consumer feature present, enforcing host cfg       | Executable checks on calls that reach them                 | Evidence for the interpreted predicates on those calls |

The default path supports a real target without `std`. Procedural macros still use the build host's standard library; that is not a target runtime dependency. Required validation and safety checks must remain ordinary code, independent of this feature.

For a native verification invocation, `RUSTFLAGS="--cfg anodized_panic"` selects enforcement when the backend's host artifact is compiled. The repository tasks both set the mode and run a deliberate violation. Cross-target flags need not configure host procedural macros, and a target cfg listing cannot certify a cached host artifact.

## Nested syntax and expansion boundary

A trait or implementation uses the qualified outer attribute and bare nested `#[spec(...)]` markers, matching the backend's syntax. Empty nested markers use `#[spec()]`. Qualified nested markers are not an additional interface.

When disabled, the wrapper removes nested markers as well as the outer annotation. Unrelated attributes and macro-language payloads retain their tokens. The double-underscore re-exports are the expansion ABI, not alternative public annotation spellings.

## Specification, evidence, and future interpretation

A specification describes admitted behavior; satisfaction relates an implementation to that description. Adding a requirement refines the specification. Checking or proving an existing requirement strengthens evidence instead. Adequacy asks whether the statements, observations, and evidence distinguish relevant deviations—not merely whether named tests pass.

Omitting executable checking changes neither the authored obligation nor its authority. It also supplies no evidence for that obligation. A future verification adapter must consume authored source or a deliberately preserved representation and relate it to the selected build. Removed clauses are not presumed to remain in stripped HIR, and this facade establishes no cross-verifier correspondence or full-abstraction theorem.

Upgrades must exercise the selected package's actual source, generated interface, and both consumer configurations. Matching version labels alone do not establish compatibility. The exact published macro pin is intentional until that evidence is available.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
