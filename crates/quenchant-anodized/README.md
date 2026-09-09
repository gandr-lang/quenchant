# quenchant-anodized

One authored specification, with executable checking selected by the consuming crate. The facade itself is `no_std`; its optional backend runs on the build host and emits code requiring `std` in an instrumented consumer.

## Install

Use the dependency name `quenchant`: the expansion addresses helpers through that name.

```toml
[dependencies]
quenchant = { package = "quenchant-anodized", version = "=0.1.0", path = "../quenchant-anodized" }

[features]
anodized = ["quenchant/anodized"]
```

The path selects a workspace sibling. Remove it for registry installation after this package and its macro dependency are published.

## Example

```rust
# extern crate quenchant_anodized as quenchant;
#[quenchant::spec(ensures: |output| output.is_ok())]
fn accept() -> Result<(), core::convert::Infallible> {
    Ok(())
}

assert_eq!(accept(), Ok(()));
```

The consuming crate declares the `anodized` feature and uses `std` in that configuration. Without it, the annotation and supported nested markers disappear; required ordinary validation and the function body remain. With it, the original item and predicate tokens reach published `anodized-macros` 0.6.0.

## Enforcement and nested markers

Backend selection and enforcement are separate. On a native verification invocation, `RUSTFLAGS="--cfg anodized_panic"` selects violation panics when the host macro artifact is compiled. The workspace verification task supplies this setting and exercises an enforcement witness. Cross-target compiler flags need not affect host procedural macros; a target cfg listing alone cannot certify the loaded backend.

An outer `#[quenchant::spec]` may contain bare `#[spec(...)]` markers on supported nested items. The disabled route removes those markers too. Qualified nested markers are not an extension offered here. Other attributes and macro-language payloads are preserved.

## Evidence boundary

The authored specification remains authoritative in either mode. An executed check supplies evidence only for its interpreted predicate on that call. Omitted checking supplies none. Types, specification statements, satisfaction, evidence, and adequacy are not interchangeable claims.

A future verification adapter must consume authored source or a preserved specification representation and relate it to the selected build. Stripped HIR is not presumed to contain erased clauses. The facade implements conditional expansion, not such an adapter or a proof-transfer theorem.

The double-underscore re-exports are the expansion ABI, not alternative public annotation spellings. No Anodized runtime or logic implementation is copied into this crate. Macro upgrades must exercise the actual expansion, both feature configurations, and any changed helper requirements; equal version labels are not compatibility evidence.
