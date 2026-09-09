# quenchant-fixture-macros

A separate procedural-macro crate used as foreign code by the Dylint UI matrix. Its generated declarations let the matrix distinguish an authored method from a generated sibling that happens to reuse the author's identifier token.

This package is fixture infrastructure, not a runtime client generator. `publish = false` is part of its package boundary.

## Install for a local fixture

From another crate in this workspace:

```toml
[dev-dependencies]
quenchant-fixture-macros = { path = "../quenchant-fixture-macros", version = "=0.0.0" }
```

Normal consumers do not need this package. The Dylint harness links the actual compiled macro artifact into its separately compiled fixtures.

## Example

```rust
struct Endpoint;

#[quenchant_fixture_macros::client]
impl Endpoint {
    fn request(&self) {}
}

Endpoint.request();
EndpointClient.request();
```

`client` preserves the original item and adds an `EndpointClient` sibling. Generated methods reuse original method-name tokens while their declarations are manufactured by the macro. Their empty bodies and missing documentation are intentional test data, not a production fallback.

The supported input is the simple inherent-implementation shape used by these fixtures. An item without a recognizable implementation subject and body is emitted unchanged. The macro does not claim to parse or implement an arbitrary client API.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
