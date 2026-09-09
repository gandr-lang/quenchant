# quenchant

The consumer namespace for the quenchant libraries. At `0.0.0` the namespace is the specification facade alone: a consumer adds this package and writes `#[quenchant::spec(...)]` without naming the backend package. Every item is a re-export and this package declares no interface of its own.

`0.0.1` widens the namespace to `quenchant::arith`, `quenchant::shape`, the crate-root macros `reason_enum!`, `nominal_type!`, and `delegate_ops!`, and `quenchant::gates` behind a feature of that name. A consumer that wants those at `0.0.0` depends on `quenchant-arith`, `quenchant-shape`, and `quenchant-gates` by name.

## Install

From an application beside a workspace checkout:

```toml
[dependencies]
quenchant = { version = "=0.0.0", path = "../quenchant/crates/quenchant" }

[features]
anodized = ["quenchant/anodized"]
```

After publication, the same version can be selected without the path. The default build is `no_std`.

## Example

```rust
#[quenchant::spec(ensures: |ref output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}

assert_eq!(accept(), Ok(()));
```

The attribute resolves through this package because the expansion writes `::quenchant::` helper paths. That is the same reason a consumer of `quenchant-anodized` alone names that dependency `quenchant`.

## What the namespace contains

| Path              | Package              | Contents                                              |
| ----------------- | -------------------- | ----------------------------------------------------- |
| `quenchant::spec` | `quenchant-anodized` | The specification attribute and its expansion helpers |

## Features

| Feature    | Effect                                                                    |
| ---------- | ------------------------------------------------------------------------- |
| `anodized` | Selects the published specification backend in the facade; requires `std` |

The `anodized` feature must also be declared in the consuming crate: the expansion tests a consumer-side condition, and enabling a dependency feature is not a substitute for it.

## What the namespace omits

`quenchant-arith` and `quenchant-shape` carry the arithmetic and shape libraries. They are named directly at this version; `0.0.1` re-exports them as `quenchant::arith` and `quenchant::shape`.

`quenchant-gates` reads Cargo and nextest output and requires `std`, so it stays behind a feature rather than joining the default namespace.

`quenchant-dylints` is a compiler plugin. Dylint loads its `cdylib` from a path or a Git revision paired with the matching compiler, so no Rust crate links it and no re-export can stand in for that pairing.

`quenchant-spec-macros` is the token-forwarding implementation behind the specification attribute. It is reached through `quenchant::spec`, never by name.

`quenchant-fixture-macros` stages foreign expansions for the compiler-plugin fixtures and is never published.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
