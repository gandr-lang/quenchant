# quenchant-arith

Choose arithmetic semantics by name rather than by optimization profile. `Int<Representation>` gives a nominal boundary around each standard signed or unsigned machine integer, including pointer-sized integers. Its representation is private and transparent; the sealed `Integer` trait admits the supported representations only.

## Install

From an application beside a workspace checkout:

```toml
[dependencies]
quenchant-arith = { version = "=0.0.0", path = "../quenchant/crates/quenchant-arith" }
```

After publication, the same version can be selected without the path. The default library is `no_std` and requires no allocator.

## Example

```rust
use quenchant_arith::arith::{self, ArithmeticError, Int, Operation};

let left = Int::from(250_u8);
let right = Int::from(10_u8);
assert_eq!(
    arith::checked_add(left, right),
    Err(ArithmeticError::Overflow(Operation::Add)),
);
assert_eq!(arith::wrapping_add(left, right), Int::from(4_u8));
assert_eq!(arith::saturating_add(left, right), Int::from(u8::MAX));
```

The error preserves both the operation and the cause. Division and remainder distinguish a zero divisor from signed overflow; a caller does not have to infer the failure from a Boolean or absent value.

## Arithmetic families

Each family provides addition, subtraction, multiplication, division, and remainder:

| Entry points                                 | Overflow                           | Zero divisor                   | Safety |
| -------------------------------------------- | ---------------------------------- | ------------------------------ | ------ |
| `strict_*`                                   | Panic in debug and release         | Panic                          | Safe   |
| `checked_*`                                  | `ArithmeticError::Overflow`        | `ArithmeticError::ZeroDivisor` | Safe   |
| `wrapping_*`                                 | Width-modular result               | Panic                          | Safe   |
| `saturating_*`                               | Clamp to the representation bounds | Panic                          | Safe   |
| Unprefixed operations, default configuration | Strict behavior                    | Panic                          | Safe   |
| Unprefixed operations with `fast`            | Caller must exclude it             | Caller must exclude it         | Unsafe |

Signed division truncates toward zero. Signed `MIN / -1` and `MIN % -1` violate the unchecked precondition even though the mathematical remainder is zero. Wrapping division returns the width-modular result for the signed overflow pair; saturation clamps division and uses the representable remainder for every nonzero divisor.

**Enabling `fast` changes the unprefixed functions into unsafe entry points.** Callers must establish that the corresponding checked operation succeeds. Do not deliberately violate that precondition to test it. Explicitly strict functions and the standard operator implementations on `Int` remain safe and strict in every configuration.

## Specification instrumentation

The independent `anodized` feature enables the published specification backend and requires `std`. It does not change the arithmetic family selected by `fast`.

To exercise the enforcing configuration from the workspace root:

```sh
RUSTFLAGS="--cfg anodized_panic" mise exec -- cargo test -p quenchant-arith --features anodized
```

The annotated predicates compare the selected arithmetic relation on executed calls. They do not prove all inputs correct, and their removal in the default `no_std` configuration removes no ordinary arithmetic validation. The trait-level type restriction, authored clauses, and runtime observations have different evidence scopes.

The workspace gates separately exercise strict/fast and debug/release behavior, boundary relations, real bare-metal compilation, and valid-input unchecked operations under Miri. These are the tested routes, not an unbounded equivalence or mutation-adequacy claim.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
