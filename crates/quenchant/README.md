# quenchant

One namespace over the publishable quenchant libraries. A consumer adds this package and reaches `quenchant::arith`, `quenchant::shape`, and the `#[quenchant::spec(...)]` attribute without naming each library separately. It re-exports its members and declares no interface of its own.

## Install

From an application beside a workspace checkout:

```toml
[dependencies]
quenchant = { version = "=0.0.1", path = "../quenchant/crates/quenchant" }

[features]
anodized = ["quenchant/anodized"]
```

After publication, the same version can be selected without the path. The default build is `no_std`. A consumer that wants one library alone can depend on that package directly; the two arrangements select the same code.

## Example

```rust
use quenchant::arith::{self, ArithmeticError, Int, Operation};
use quenchant::shape::Maybe;

quenchant::reason_enum! {
    pub mod lookup {
        #[derive(Debug, Eq, PartialEq)]
        pub enum Unavailable { NotFound }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Label { Original }

#[quenchant::spec(ensures: |ref output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}

assert_eq!(
    arith::checked_add(Int::from(250_u8), Int::from(10_u8)),
    Err(ArithmeticError::Overflow(Operation::Add)),
);

let absent: Maybe<Label, lookup::Unavailable> = Maybe::Absent(lookup::Unavailable::NotFound);
assert_eq!(absent, Maybe::Absent(lookup::Unavailable::NotFound));

assert_eq!(accept(), Ok(()));
```

The attribute resolves through this package because the expansion writes `::quenchant::` helper paths. That is the same reason a consumer of `quenchant-anodized` alone names that dependency `quenchant`.

## What the namespace contains

| Path               | Package              | Contents                                                                       |
| ------------------ | -------------------- | ------------------------------------------------------------------------------ |
| `quenchant::arith` | `quenchant-arith`    | Nominal integers and the named arithmetic families                             |
| `quenchant::shape` | `quenchant-shape`    | Reason-preserving absence and transparent domain types                         |
| `quenchant::spec`  | `quenchant-anodized` | The specification attribute and its expansion helpers                          |
| Crate-root macros  | `quenchant-shape`    | `reason_enum!`, `nominal_type!`, and `delegate_ops!`                           |
| `quenchant::gates` | `quenchant-gates`    | Invocation-state and adequacy-witness reporting, behind the `gates` feature    |

## Features

| Feature    | Effect                                                                                   |
| ---------- | ---------------------------------------------------------------------------------------- |
| `anodized` | Selects the published specification backend in every re-exported library; requires `std` |
| `gates`    | Adds `quenchant::gates`; that library reads Cargo and nextest output and requires `std`  |

The `anodized` feature must also be declared in the consuming crate: the expansion tests a consumer-side condition, and enabling a dependency feature is not a substitute for it.

## What the namespace omits

`quenchant-dylints` is a compiler plugin. Dylint loads its `cdylib` from a path or a Git revision paired with the matching compiler, so no Rust crate links it and no re-export can stand in for that pairing.

`quenchant-spec-macros` is the token-forwarding implementation behind the specification attribute. It is reached through `quenchant::spec`, never by name.

`quenchant-fixture-macros` stages foreign expansions for the compiler-plugin fixtures and is never published.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
