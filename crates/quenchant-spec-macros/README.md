# quenchant-spec-macros

Host-side attribute expansion for `quenchant-anodized`. This package has no normal dependencies. It emits a consumer-side feature selection and removes supported nested specification markers when instrumentation is disabled.

## Install

Applications install the facade, not this implementation package:

```toml
[dependencies]
quenchant = { package = "quenchant-anodized", version = "=0.1.0", path = "../quenchant-anodized" }

[features]
anodized = ["quenchant/anodized"]
```

The path selects a workspace sibling. Registry-only installation is available after the packages are published.

## Example

The facade provides the attribute path and its expansion helpers:

```rust
# extern crate self as quenchant;
# pub use quenchant_spec_macros::{spec, __erase};
# #[cfg(feature = "anodized")]
# pub use anodized_macros::spec as __instrument;
#[quenchant::spec(ensures: |output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}
# fn main() {
assert_eq!(accept(), Ok(()));
# }
```

`anodized` is evaluated in the consuming crate. When enabled, the original tokens reach the published backend. Otherwise the outer attribute and bare nested `spec` markers disappear; ordinary code remains. Nested qualified markers are not part of this interface.

## Expansion boundary

The stripping pass walks token groups with an explicit stack. Other attributes, macro definitions, and macro invocation payloads retain their tokens: an attribute-shaped sequence inside a macro can be data rather than an annotation owned by this pass. Original item tokens retain their spans; reconstructed delimiter groups use the original group span.

`__erase` is an expansion helper, not an alternate annotation surface. It rejects arguments. Specification erasure supplies no verification evidence and must never remove required ordinary validation. Neither this package nor its facade proves correspondence to a separate verification backend.
