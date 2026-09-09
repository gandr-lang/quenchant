# quenchant-spec-macros

Host-side expansion for the [quenchant-anodized facade](../quenchant-anodized/README.md). The normal dependency graph is empty: the wrapper uses Rust's supplied token API rather than a second specification parser. The published backend is a development dependency for integration tests, not a dependency of disabled consumer builds.

## Install through the facade

From an application beside a checkout:

```toml
[dependencies]
quenchant = { package = "quenchant-anodized", version = "=0.0.0", path = "../quenchant/crates/quenchant-anodized" }

[features]
anodized = ["quenchant/anodized"]
```

Applications use `#[quenchant::spec]`; direct installation of the implementation package does not provide the facade's helper namespace. Registry-only installation follows publication of the package family.

## Example

```rust
# extern crate self as quenchant;
# pub use quenchant_spec_macros::{spec, __erase};
# #[cfg(feature = "anodized")]
# pub use anodized_macros::spec as __instrument;
#[quenchant::spec(ensures: |ref output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}
# fn main() {
assert_eq!(accept(), Ok(()));
# }
```

The hidden setup supplies the facade namespace for this package's executable documentation. In an application, the facade dependency supplies it.

## Selection belongs to the consumer

`spec` emits two mutually exclusive `cfg_attr` routes. The consuming crate's `anodized` feature chooses published instrumentation or `__erase`. The wrapper's host-side compilation cannot decide that consumer condition correctly by inspecting only its own dependency features.

The enabled route forwards the original item and predicate tokens. It does not reinterpret the predicate language. Selection alone does not establish enforcement; the facade guide describes the separate host cfg and execution witness.

## Removing markers without rewriting the program

`__erase` uses an explicit stack of delimiter frames. It removes the bare nested `spec` attribute form, not qualified paths or arbitrary metadata. It does not evaluate predicates, replay callbacks, or remove ordinary validation.

Tokenization gives balanced groups, not a ready-made map of which attributes belong to this interpretation. The traversal therefore distinguishes item code from macro definitions and invocation payloads. An attribute-shaped sequence inside a macro can be data; descending blindly into every group would change that macro's language.

Ordinary tokens keep their order and spans. Rebuilt groups use the original enclosing span. The helper rejects arguments because it is not a second entry point for interpreting specification clauses. Runtime allocations, compiler invocation overhead, and incremental build behavior are separate from the token algorithm; no measured overhead bound is claimed here.

## Verification scope

Integration cases run the real host macro API through consumer compilation. They distinguish disabled nested-code preservation, untouched macro matcher and invocation payloads, inherited trait predicates, move-only early returns, and backend selection without enforcement. The library packages also compile for a target without a standard library.

These witnesses defend the adapter boundary. They do not establish that an arbitrary authored specification is complete, that the backend proves it, or that another verification target has equivalent semantics.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
