# quenchant-gates

The project's non-lint gates: the workflow obligations that a Dylint pass cannot decide, because deciding them needs more than one crate at a time.

A lint pass sees one crate's HIR. Some invariants are not in the HIR at all — they live in the resolved build graph, in the test inventory, or in the shape of the workspace. Those checks live here and run as ordinary binaries the wall invokes, beside `quenchant-dylints` rather than inside it.

The crate is a workspace member and carries no dependency on any language crate.

## Provision

- **Specification-enforcement state.** Reports whether the consumer invocation discards specifications, enforces them through panics, only prints violations, or leaves checks non-enforcing.

- **G0, adequacy witness resolution.** Every `- witness:` path in the workspace resolves to exactly one runnable test in the item's **own crate's** targets. Absent, renamed, ambiguous and wrong-target paths fail, each reported at the file and line of the bullet that must change.

The companion grammar rule for G0 is the `adequacy_block_grammar` gate in `quenchant-dylints`, which admits a bullet only in the exact form this gate resolves. The two read the same section under the same terminating rule.

## Enforcement state

`anodized-macros` chooses instrumentation when its host artifact is built. The gate queries `cargo rustc -Z unstable-options --print cfg` from the consumer manifest's directory, allowing Cargo to resolve configuration, `RUSTFLAGS`, and higher-priority `CARGO_ENCODED_RUSTFLAGS`. It never reads the installed gate binary's compile-time cfgs.

| Resolved cfg                                   | State           | Observation lane | Enforcing lane |
| ---------------------------------------------- | --------------- | ---------------- | -------------- |
| `anodized_discard_specs`, with any other flags | `discarded`     | fail             | fail           |
| `anodized_panic`, without discard              | `enforcing`     | pass             | pass           |
| `anodized_print`, without panic or discard     | `print-only`    | pass             | fail           |
| None of these                                  | `non-enforcing` | pass             | fail           |

State concerns this invocation, not already-built dependencies. Explicit targets can separate target flags from host proc-macro flags. Every enforcing consumer lane must also execute a runtime specification sentinel built with its graph-wide flags. The gate does not claim that a target cfg report proves host instrumentation or invalidates cached artifacts.

A missing manifest, failed Cargo query, invalid output encoding, or absent compiler target evidence is an operational error. A failed measurement never becomes a passing policy result.

## Running it

```console
cargo run -p quenchant-gates -- anodized --manifest-path Cargo.toml
RUSTFLAGS="--cfg anodized_panic" cargo run -p quenchant-gates -- anodized --manifest-path Cargo.toml --require-enforcing
cargo run -p quenchant-gates -- witnesses --manifest-path Cargo.toml
```

Each form requires the consumer workspace manifest. The specification gate succeeds according to the table above; `witnesses` succeeds when every declared witness resolves. Either gate reports operational failure separately from a measured policy finding.

The witness command runs G0. Its exit status is zero when every witness resolves, non-zero when any does not, and non-zero with a distinct message when the gate could not reach a verdict.

G0 runs in the Dylint CI job because it lists `quenchant-dylints`'s tests too, and that listing needs `dylint-link`.

`cargo test -p quenchant-gates` checks every enforcement-mode combination, both lane requirements, exact cfg-name boundaries, and missing compiler evidence. G0 resolves recorded listings, including unsupported listings and unparseable source refused as operational errors. Live invocation checks exercise the actual binary and Cargo query.

## What a witness must name

A witness is resolved against the **owning crate's** inventory, keyed by the exact alias the test harness reports for it:

| Target                       | The alias a test contributes                             |
| ---------------------------- | -------------------------------------------------------- |
| library or binary            | the module path from the crate root, unprefixed          |
| integration test             | the target name, then the module path within it          |

So a unit test in `src/memo.rs` is `memo::tests::an_ordered_memo_serves_what_it_was_told`, a test in `tests/acceptance.rs` is `acceptance::memoized_checking_agrees_with_memoless`, and a test in a consolidated suite whose target is named `tests` is `tests::store::a_written_root_opens`.

The target prefix is load-bearing rather than decorative. Two integration targets of one crate may both declare `fn round_trips()`, and a witness naming neither has not told a reviewer which test to watch fail. A path resolving to more than one target of the crate is reported as ambiguous.

Findings carry the repair when the inventory offers one. A path naming a real test under a target that does not hold it reports the target that does; a path that runs only in a sibling crate names the sibling, because a witness must name a test in the item's own crate.

A block carrying `- declaration-only:` contributes no obligation — it asserts there is nothing to witness. Whether that exemption was allowed is the Dylint gate's decision, not this one's.

## Where witnesses are read from

Bullets are read from **parsed** declarations, not from raw text: a `- witness:` line inside a doc code fence, a string literal or an ordinary comment is not an obligation, and a text scan cannot tell the difference. The files read are those under the source directories Cargo declares for the package's own targets, so a fixture corpus a harness compiles separately — the Dylint UI matrix — is data rather than source. A build script is the one target that contributes nothing: its source sits in the member's own directory, and expanding it into a source root would sweep those data trees back in, so it is excluded from the read scope.

The `# Adequacy` section runs from its heading to the next heading of any level. A reader that ran to the end of the doc block would absorb whatever a rustdoc-injecting attribute macro appends after the author's prose.

## The test inventory

The inventory is built from `cargo nextest list --message-format json` when `cargo-nextest` is on `PATH`. Without it the gate falls back to `cargo test --no-run --message-format json` and asks each built test binary for `--list --format terse`; both routes preserve the owning package and target of every test, which is what makes a wrong-target path distinguishable from a correct one.

**A workspace member that pins its own toolchain is listed from its own directory.** A `rust-toolchain` file governs the directory a command runs in, not the package a command names, so such a member is excluded from the workspace-wide listing and listed separately, and the two listings are merged. No member pins its own toolchain — the gate library builds under the workspace's single pinned toolchain — so the gate lists the whole workspace in one run; the alternative is to drop a pinning member from the gate, which is the same as declaring its witnesses exempt.

An inventory that listed no test at all is refused rather than returned. A gate that measured nothing must not report a pass.

## License

Apache-2.0 OR Apache-2.0 WITH LLVM-exception.
