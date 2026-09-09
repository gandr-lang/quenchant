# Local CI with act

Run `.github/workflows/ci.yml` locally through [act](https://github.com/nektos/act) and Docker. This is the iteration loop for workflow changes before hosted CI observes the repository settings and selected-Actions policy that a local runner cannot see.

## Inline `run:` rule

An inline `run:` step is one command. Anything with a branch, loop, or multiple pipelines lives under `scripts/ci/`; the workflow supplies inputs through `env:` and invokes the script without embedding expressions in shell source.

The project-owned scripts are:

- `scripts/ci/check-pins.sh` — compares the nightly, rust-clippy tag, Dylint crate versions, and installed Dylint tool versions; with consumer inputs it also compares an external consumer's Dylint library and gate-binary Git revisions.
- `scripts/ci/check-publish.py` — reads `cargo metadata` and refuses every package that does not report the empty publish allowlist produced by `publish = false`.
- `scripts/ci/check-public-boundary.sh` — rejects private-material shapes in tracked files and commit messages without publishing a private-name deny list.
- `scripts/ci/check-action-pins.py` — refuses a GitHub Action reference that is not an allowed action pinned to a full commit SHA.
- `.github/actions/setup-rust` — restores and saves rustup state, installs the pinned toolchain, and shares the Rust dependency cache across jobs.

## Requirements

- Docker's daemon is running (`docker info` succeeds).
- `act` is on `PATH`; `mise.toml` pins version `0.2.89`.
- `mise install` has resolved the repository tools.

## Invocation

The root `.actrc` pins the Linux image and disables repeated pulls. Each hosted context has one local job:

```sh
act pull_request -j build-test
act pull_request -j dylint
act pull_request -j clippy
act pull_request -j policy
```

List jobs without running them:

```sh
act -l
```

Time a job for relative before/after comparison on one host:

```sh
time act pull_request -j dylint
```

## Toolchain bump

The whole workspace builds under `rust-toolchain.toml`. `mise run toolchain:bump <stable>` reads the matching rust-clippy release tag, changes the nightly and `clippy_utils` tag together, and prints the driver rebuild command. Remove `~/.dylint_drivers` and `target/dylint`, rebuild the driver, then run every repository gate.

## Platform notes

- act executes these Linux jobs in the image pinned by `.actrc`; Docker chooses a native image manifest where one exists.
- act's built-in cache server supports the workflow's `actions/cache` steps without extra flags.
- Supply a GitHub token only when the pin check would otherwise hit anonymous API limits. Pass it through the environment, never as a literal command argument.
- A hosted `startup_failure` caused by a repository Actions allowlist is invisible to strict YAML parsing and act. A pushed pull request whose four jobs register is the only proof of that settings boundary.
- Local wall time is useful only for relative comparisons on the same host; hosted CI timing remains the hosted-run observation.
