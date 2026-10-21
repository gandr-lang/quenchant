# quenchant-gates

Source-level, invocation-level, and repository verification outside compiler lints. The executable reports Anodized cfg state, resolves consumer adequacy witnesses, reports the re-verification a change owes, and refuses repository boundary, pin, action, and publication violations.

## Install

From the workspace root:

```sh
mise exec -- cargo install --path crates/quenchant-gates --locked
```

Use the repository's pinned toolchain. The invocation-state query requires nightly Cargo support; the installed binary cannot make a stable consumer's compiler expose an unsupported query. A consumer should keep this executable and its chosen Dylint library at compatible source revisions.

This package is distributed through Git, with `publish = false`. External consumers install a reviewed full revision with `cargo install --git https://github.com/gandr-lang/quenchant --rev <revision> --locked --bin quenchant-gates quenchant-gates`; the umbrella does not link its library.

## Example

Run directly from source without installing:

```sh
mise exec -- cargo run -p quenchant-gates --locked -- anodized --manifest-path Cargo.toml
mise exec -- cargo run -p quenchant-gates --locked -- witnesses --manifest-path Cargo.toml
```

The consumer manifest is mandatory for `anodized` and `witnesses`. An installed tool must inspect the consumer the invocation names, not the tool's original checkout or an accidentally selected directory.

## Invocation-state evidence

The `anodized` command asks Cargo for the compiler cfgs resolved at the consumer manifest. It distinguishes compile-only, print-only, panic-enforcing, embedded, discarded, and incompatible modes. Discard conflicts with every other mode; embedding conflicts with runtime checks; deferred failures require panic mode. Exact cfg names matter.

Add `--require-enforcing` to require panic enforcement. The repository's corresponding task also executes a deliberately violated specification. That execution is necessary because target cfg output alone does not certify a host procedural-macro artifact, its features, or its cache state.

The facade's feature compiles the backend; the build cfg selects instrumentation. This command reports the cfg mode, not a census of functions carrying emitted checks. Omitting instrumentation does not erase authored obligations. Direct backend discard remains unacceptable for a lane claiming enforcement.

## Adequacy witnesses

The `witnesses` command discovers package ownership and source roots, obtains a runnable test inventory, and resolves each declared witness within its owning package. Missing, ambiguous, cross-package, and wrong-target references produce addressed findings. A failed inventory query or unparseable source is an operational error, never an empty successful result.

Runner selection belongs to the supplied consumer workspace. Rustup is required: metadata discovery, runner probing/listing, and fallback native-binary listing use the source-resolved compiler through `rustup run`, preserving its complete proxy environment. Inherited producer `CARGO` and `RUSTUP_TOOLCHAIN` values do not select the consumer's compiler. For each nonempty compiler scope, the gate resolves `mise which cargo-nextest` there and retains that absolute executable through its version probe and JSON listing, without Cargo's global plugin search.

Mise-managed scopes require a selected, installed nextest. A failed selection, malformed executable path, failed probe, or failed listing is an operational error; no global nextest is substituted. When mise itself is unavailable, the ordinary `cargo test --no-run` plus native test-binary listing route remains supported. A workspace containing only member-local compiler scopes has no shared listing to execute; each member is still inventoried, and a final inventory containing no tests is refused.

A resolved name proves that the named test is available under the inventoried configuration. It does not prove that the test ran in another configuration, that its oracle is adequate, or that it establishes the item's complete specification. The authored hypothesis must name the inputs, observations, fault classes, and remaining scope.

## Re-verification obligations

When an item changes, the specifications that name it owe re-verification. `obligations` computes what a working tree owes against a base revision:

```sh
quenchant-gates obligations --manifest-path Cargo.toml --base "$(git merge-base origin/main HEAD)" \
  --records target/evidence --rustdoc target/doc \
  --body body.md --comment comment.md --selection selection.args --require-acknowledged
```

It reads two inputs the consumer builds first, both from the working tree:

- the records [quenchant-dylints](../quenchant-dylints/README.md#specification-dependency-records) writes when its configuration names an `evidence_directory`: every item with its extent and revision, and the items each executable `#[spec]` clause names, resolved by the compiler;
- rustdoc's JSON output, `RUSTDOCFLAGS="-Z unstable-options --output-format json" cargo doc --workspace --no-deps --document-private-items`, whose resolved intra-doc links supply the prose edges. A link counts when it sits in a `# Specification` bullet or an `# Errors`, `# Panics`, `# Safety` or `# Termination` section.

An item changed when `git diff --relative --no-renames -U0 <base>` touches its extent; untracked files are not seen. A specification owes one obligation per clause that names a changed item, and its witnesses rerun when it is obligated or changed itself. Only named dependencies count, never the call graph, and obligations stop one hop from the changed items: a specification that re-verifies unchanged flags nothing that names it. A rename has no identity across it today: the item reads as new under its new path, and every specification naming it had to change to compile, so it reads as changed too.

The console report lists changed items, obligations and witnesses. `--selection` receives `cargo nextest run` arguments, one per line, that run exactly the obligated witnesses.

`--body` names a pull-request body. The command reads its acknowledgements and writes it back with a re-verification section between two markers:

- a heading that carries the verdict: `## Re-verification · ✅ 9 of 9 settled`, `⚠️ 2 of 9 unticked`, or `✅ nothing owed`;
- a table with one row per prose obligation: item, clause, names, verdict;
- a "still holds" box and an "updated" box per obligation, collapsed under `<details>`.

A ticked box carries forward while the specification and the changed items keep the revisions it was ticked at. `--previous` reads acknowledgements from another file instead, for a reader that must not rewrite the body.

`--comment` receives a pull-request comment with the same heading and table, and the witnesses rerun collapsed beneath them. `--require-acknowledged` exits with status 2 while any prose obligation is open, naming each with its key.

The module documentation in [`src/obligations.rs`](src/obligations.rs) and [`src/evidence.rs`](src/evidence.rs) records the decisions and their reversal conditions.

## Repository checks

These commands accept `--root <repository>`; omission selects the working directory. Run through `mise exec -- cargo run --quiet --locked -p quenchant-gates -- <command>` so Cargo and its instruments use the pinned toolchain.

| Command | Refusal boundary |
| ------- | ---------------- |
| `public-boundary` | Tracked control directories, private material in tracked text and commit messages, private contributor email. Opaque commit provenance remains permitted; tracked session tokens do not. |
| `pins` | Missing or drifted compiler/Dylint pins; malformed stable tags; unequal or incomplete consumer revisions. |
| `action-pins` | External YAML action references outside the allowlist or without a full lowercase 40-hex revision. Local actions remain permitted. |
| `publish-allowlist` | Missing, unclassified, or incorrectly publishable packages against the single declared package boundary. |
| `conflict-markers` | Exact seven-character Git conflict markers in tracked text. |

`pins` reads the exact rust-clippy release through `gh api`. `--upstream-toolchain <file>` explicitly substitutes offline evidence. `WORKSPACE_MANIFEST`, `TOOL_CONFIG`, and `TOOLCHAIN_FILE` retain pin-input overrides. Optional `--consumer-manifest <file>` and `--consumer-config <file>` must appear together. Relative inputs resolve under `--root`.

`toolchain-bump --version <stable>` reads upstream evidence before updating only `clippy_utils.tag` and the compiler channel, preserving other TOML fields and comments. `mise run toolchain:bump <stable>` launches it. Rebuild the Dylint driver and rerun the gate set after a compiler move.

Each check carries a `# Specification`, closed refusal reasons in `Maybe`, and addressed operational errors. Deliberately invalid repository fixtures exercise the CLI as well as the pure comparisons. Git-backed checks inspect tracked working-tree text, not untracked files or historical file contents.

## Executable specifications

Executable predicates check catalog preservation, witness resolution, invocation policy, pin coherence, and typed failure boundaries. Formatter and process boundaries carry explicit exemptions where the returned value omits the observation needed for an independent runtime check. Their adequacy witnesses exercise the observable boundary instead.

`mise run check:dylint` enforces the five executable-specification policy lints across the library, binary, and integration-test targets through workspace-wide flags. Specifications use the defaults-off `quenchant-anodized` facade; `quenchant-anodized/anodized` enables the backend, and the enforcing tasks select `anodized_panic` for the complete build graph.

## Results

A successful exit means the selected gate completed and found no violation. Policy findings and operational failures both produce unsuccessful exits, with different diagnostics explaining whether a verdict was reached. The CLI does not promise a distinct numeric exit code for every failure class.

The library exposes the catalog, inventory, witness-resolution, and cfg-classification types used by the driver. Those interfaces preserve package/target identity and operational errors rather than reducing them to a success bit.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
