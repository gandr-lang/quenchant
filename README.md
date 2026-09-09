# quenchant

Unpublished Rust core libraries and the workspace policy that gates them, in one workspace under one toolchain pin.

## Crates

- [`quenchant-arith`](crates/quenchant-arith/README.md) provides profile-independent integer arithmetic at nominal type boundaries: the same specification in debug and release, with an opt-in `fast` profile.
- [`quenchant-shape`](crates/quenchant-shape/README.md) provides nominal non-failure absence and transparent type scaffolding, `Maybe<T, R>` among them.
- [`quenchant-dylints`](crates/quenchant-dylints/README.md) provides Dylint rules for policy that Clippy cannot express.
- [`quenchant-gates`](crates/quenchant-gates/README.md) reports specification-enforcement state and resolves adequacy witnesses.
- [`quenchant-fixture-macros`](crates/quenchant-fixture-macros/README.md) supplies the attribute macros the Dylint UI matrix expands, so a rule about foreign expansions is read against a real one.

Every package carries `publish = false`. The two libraries are licensed `MIT OR Apache-2.0`; the three policy crates carry `Apache-2.0 OR Apache-2.0 WITH LLVM-exception`, because the Dylint library links the compiler's internal libraries.

## The self-hosted wall

The Dylint policy library is a member of this workspace and is loaded by path:

```toml
[[workspace.metadata.dylint.libraries]]
path = "crates/quenchant-dylints"
```

A lint and the code it governs therefore change in one commit, and no revision pin stands between them. An external consumer loads the same library by revision:

```toml
[[workspace.metadata.dylint.libraries]]
git = "https://github.com/silvanshade/quenchant"
rev = "<full 40-hex commit>"
pattern = "crates/quenchant-dylints"
```

Install the gate binary from the same revision:

```sh
cargo install \
  --git https://github.com/silvanshade/quenchant \
  --rev <same full 40-hex commit> \
  --locked \
  --bin quenchant-gates \
  quenchant-gates
```

Invoke every gate with the consumer manifest explicitly:

```sh
quenchant-gates anodized --manifest-path Cargo.toml
RUSTFLAGS="--cfg anodized_panic" quenchant-gates anodized --manifest-path Cargo.toml --require-enforcing
quenchant-gates witnesses --manifest-path Cargo.toml
```

The consumer toolchain must match this repository's pinned nightly. `cargo-dylint` and `dylint-link` must match the Dylint crate version; `mise run check:ci-pins` verifies the repository half of that relation.

The state gate reads the consumer invocation's resolved cfgs. It rejects discarded specifications; `--require-enforcing` also rejects non-panicking modes. Enforcing lanes must execute a runtime specification sentinel as well, because target cfgs alone cannot certify host proc-macro artifacts.

## Development

```sh
mise install
mise run check
```

The Dylint package's test and Clippy commands run from `crates/quenchant-dylints`, where its linker configuration applies, and the workspace passes exclude it for that reason. See [`docs/agents/source-workflow.md`](docs/agents/source-workflow.md) for the complete gate map.

## License

`MIT OR Apache-2.0` for `quenchant-arith` and `quenchant-shape`; `Apache-2.0 OR Apache-2.0 WITH LLVM-exception` for `quenchant-dylints`, `quenchant-gates`, and `quenchant-fixture-macros`. Each package states its own `license` field rather than inheriting one, so the difference is visible where it applies, and both file sets are tracked at the root: `LICENSE.MIT.txt`, `LICENSE.Apache-2.0.txt`, and `LICENSE.LLVM-exception.txt`.
