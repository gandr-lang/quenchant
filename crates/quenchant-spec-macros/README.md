# quenchant-spec-macros

Host-side expansion for the [quenchant-anodized facade](../quenchant-anodized/README.md). The normal dependency graph is empty: the wrapper uses Rust's supplied token API rather than a second specification parser. The pinned backend is a development dependency for integration tests, not a dependency of disabled consumer builds.

## Install through the facade

From an application beside a checkout:

```toml
[dependencies]
quenchant-anodized = { version = "=0.0.0", path = "../quenchant/crates/quenchant-anodized" }

```

Applications import `anodized::spec`; direct installation of the implementation package does not provide the facade's helper namespace. Registry-only installation follows publication of the package family.

## Example

```rust
# #[cfg(any(anodized_panic, anodized_print))]
# extern crate anodized as backend;
# extern crate self as anodized;
# #[cfg(any(anodized_panic, anodized_print))]
# pub use backend::{__, result, types};
# pub use quenchant_spec_macros::{spec, __erase};
# #[cfg(any(anodized_panic, anodized_print))]
# pub use anodized_macros::spec as __instrument;
#[anodized::spec(ensures: |ref output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}
# fn main() {
assert_eq!(accept(), Ok(()));
# }
```

The hidden setup supplies the facade namespace for this package's executable documentation. In an application, the facade dependency supplies it.

## Selection belongs to the build driver

`spec` emits two mutually exclusive `cfg_attr` routes rooted at `anodized`. `any(anodized_panic, anodized_print)` selects `__instrument`; its negation selects `__erase`. Consumers declare no feature. The facade's feature only makes the backend available to the build driver.

The enabled route forwards the original item and predicate tokens without reinterpreting the predicate language. The facade guide describes host-artifact selection, the named error for a missing backend, and the execution witnesses.

## Removing markers without rewriting the program

`__erase` uses an explicit stack of delimiter frames. It removes the bare nested `spec` attribute form, not qualified paths or arbitrary metadata. It does not evaluate predicates, replay callbacks, or remove ordinary validation.

Tokenization gives balanced groups, not a ready-made map of which attributes belong to this interpretation. The traversal therefore distinguishes item code from macro definitions and invocation payloads. An attribute-shaped sequence inside a macro can be data; descending blindly into every group would change that macro's language.

Ordinary tokens keep their order and spans. Rebuilt groups use the original enclosing span. The helper rejects arguments because it is not a second entry point for interpreting specification clauses. Runtime allocations, compiler invocation overhead, and incremental build behavior are separate from the token algorithm; no measured overhead bound is claimed here.

## Verification scope

Integration cases run the real host macro API through consumer compilation. They distinguish disabled type and const preservation, nested-code preservation, untouched macro payloads, inherited trait predicates, move-only early returns, and backend selection without enforcement. The library packages also compile for a target without a standard library.

These witnesses defend the adapter boundary. They do not establish that an arbitrary authored specification is complete, that the backend proves it, or that another verification target has equivalent semantics.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
