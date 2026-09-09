# quenchant-gates

Source-level and invocation-level verification that does not belong inside a compiler lint. The executable reports Anodized compiler cfg state and resolves adequacy-witness references against the actual consumer workspace.

## Install

From the workspace root:

```sh
mise exec -- cargo install --path crates/quenchant-gates --locked
```

Use the repository's pinned toolchain. The invocation-state query requires nightly Cargo support; the installed binary cannot make a stable consumer's compiler expose an unsupported query. A consumer should keep this executable and its chosen Dylint library at compatible source revisions.

## Example

Run directly from source without installing:

```sh
mise exec -- cargo run -p quenchant-gates --locked -- anodized --manifest-path Cargo.toml
mise exec -- cargo run -p quenchant-gates --locked -- witnesses --manifest-path Cargo.toml
```

The manifest is mandatory. An installed tool must inspect the consumer the invocation names, not the tool's original checkout or an accidentally selected directory.

## Invocation-state evidence

The `anodized` command asks Cargo for the compiler cfgs resolved at the consumer manifest. The states distinguish no enforcing mode, print-only mode, panic enforcement, and discarded specifications. Discard takes precedence over panic; exact cfg names matter.

Add `--require-enforcing` to require panic enforcement. The repository's corresponding task also executes a deliberately violated specification. That execution is necessary because target cfg output alone does not certify a host procedural-macro artifact, its features, or its cache state.

The facade's optional consumer feature and the backend's cfgs are separate choices. This command reports the latter; it is not a census of which functions carry emitted checks. Omitting instrumentation through the facade does not erase authored obligations. Direct backend discard remains unacceptable for a lane claiming backend enforcement.

## Adequacy witnesses

The `witnesses` command discovers package ownership and source roots, obtains a runnable test inventory, and resolves each declared witness within its owning package. Missing, ambiguous, cross-package, and wrong-target references produce addressed findings. A failed inventory query or unparseable source is an operational error, never an empty successful result.

A resolved name proves that the named test is available under the inventoried configuration. It does not prove that the test ran in another configuration, that its oracle is adequate, or that it establishes the item's complete specification. The authored hypothesis must name the inputs, observations, fault classes, and remaining scope.

## Results

A successful exit means the selected gate completed and found no violation. Policy findings and operational failures both produce unsuccessful exits, with different diagnostics explaining whether a verdict was reached. The CLI does not promise a distinct numeric exit code for every failure class.

The library exposes the catalog, inventory, witness-resolution, and cfg-classification types used by the driver. Those interfaces preserve package/target identity and operational errors rather than reducing them to a success bit.

## License

`Apache-2.0 WITH LLVM-exception`: the [license](../../LICENSE.Apache-2.0.txt) and its [exception](../../LICENSE.LLVM-exception.txt).
