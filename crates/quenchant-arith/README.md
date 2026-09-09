# quenchant-arith

`no_std` integer arithmetic with the same specification in debug and release. Anodized supplies executable specifications and is the only direct dependency; its optional logic library is disabled. The crate is Git-sourced and not published to crates.io.

## Arithmetic specifications

All five binary operations — addition, subtraction, multiplication, truncating division, and remainder — support all twelve signed and unsigned machine widths, including `usize` and `isize`.

| Family                            | Specification                                                 | Failure                                                                             |
| --------------------------------- | ------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `add`, `sub`, `mul`, `div`, `rem` | Strict arithmetic by default                                  | Overflow or a zero divisor panics in every profile                                  |
| `strict_*`                        | Permanently strict, even with `fast`                          | Same panic specification                                                            |
| `checked_*`                       | A boundary handler receives the exact result or a typed error | `ArithmeticError::Overflow(operation)` or `ArithmeticError::ZeroDivisor(operation)` |
| `wrapping_*`                      | Modular arithmetic                                            | Division and remainder still reject a zero divisor                                  |
| `saturating_*`                    | Mathematical result clamped to the representation bounds      | Division and remainder still reject a zero divisor                                  |

Signed `MIN / -1` overflows: strict panics, checked returns overflow, wrapping returns `MIN`, and saturating returns `MAX`. Signed `MIN % -1` follows the primitive overflow specification in strict and checked modes; wrapping and saturation return zero. Remainders otherwise already fit, so saturation does not need a separate primitive operation.

Wrapping and saturation require a reason at each consumer call site: modular arithmetic or clamping must be the specification. They are not overflow recovery. A strict panic identifies an invalid upstream specification; do not catch it or silently substitute checked arithmetic at that site.

## Nominal primitive boundary

The sealed `Integer` trait, rather than twelve public width modules, keeps the named operation surface uniform. It is implemented only for `Int<u8>` through `Int<u128>`, their signed counterparts, and the two pointer-width types. `Int` is transparent; its representation is private. Standard `From` implementations own primitive ingress and egress. Arithmetic implementations and named functions are `#[inline(always)]`; Anodized adds forwarding trait methods. No heap, allocator, or global state is involved.

```rust
use quenchant_arith::arith::{self, Int};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
struct Count(usize);

impl core::ops::Add for Count {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        // Specification: the producer bounds both counts so their sum fits usize.
        Self(arith::strict_add(Int::from(self.0), Int::from(rhs.0)).into())
    }
}

assert_eq!(Count(7) + Count(2), Count(9));
```

The wrapper's `impl Add` and this crate are the only places the primitive is unpacked. `Int`'s own standard operator implementations are permanently strict: a safe operator trait cannot express an unchecked precondition.

## Explicit trust with fast

The `fast` feature is off by default. Enabling it makes the default five functions `unsafe`; existing safe calls stop compiling rather than silently acquiring undefined behavior. A caller must prove that the corresponding checked operation would return `Ok`. Both zero divisors and signed `MIN / -1` or `MIN % -1` are forbidden. Use `strict_*` when an operator trait cannot carry this proof.

Addition, subtraction, and multiplication use the primitive `unchecked_*` methods. Core has no public `unchecked_div` or `unchecked_rem` method, so those two adapters use `checked_*().unwrap_unchecked()` under the identical no-failure precondition. The optimizer may assume that the checked result is present. No default operation is replaced by a bare, release-wrapping operator.

Run the strict specification witnesses and an adequate mutation campaign before making this per-crate or per-benchmark trust decision. Miri exercises only valid fast inputs: deliberately executing an invalid unchecked input would be undefined behavior, not a useful runtime check.

## Consumption and publication boundary

Private, unpublished workspaces use the canonical Git repository, select an audited commit with Cargo's `rev` key, and retain the resulting `Cargo.lock`. Defaults remain disabled unless the unsafe `fast` specification has been established. `cargo add quenchant-arith --git https://github.com/silvanshade/quenchant --no-default-features` is the initial integration command; replace its moving source selection with the reviewed revision before landing the consumer change.

Published crates cannot consume this unpublished Git dependency graph. Copying `src/arith.rs` does not remove its Anodized dependency. A publishable source-copy path requires an audited, publishable core-only specification dependency first.

The alternatives are twelve repeated public modules or a runtime arithmetic abstraction; the sealed nominal trait keeps one surface without permitting downstream implementations to weaken it. Revisit this choice if a genuine downstream representation needs a different proof boundary. Core supplies every arithmetic implementation. Anodized is pinned to a core-only fork revision with default features disabled; revisit the fork when upstream supplies an equivalent core-only release.

## Verification and configuration

`mise run check` builds, lints, documents, and tests strict and fast modes. Each debug/release test lane has an enforcing twin using `RUSTFLAGS='--cfg anodized_panic'` across the dependency graph: Anodized selects enforcement when its proc-macro dependency compiles. A cfg-gated sentinel requires a deliberately false postcondition to produce a postcondition panic, preventing an enforcing lane from silently checking syntax alone. No Cargo feature or build script controls enforcement.

`mise run treefmt:check` checks formatting. `mise run check:miri` interprets valid fast witnesses. `mise run mutants:campaign` is a separate experiment and refuses an empty caught inventory. Macro-generated primitive borders are not a source-level cargo-mutants measurement claim; boundary witnesses exercise every width, while the campaign measures the actual non-macro mutation surface. Do not equate a clean source-level campaign with proof of all arithmetic specifications.

Specifications compare generic functions against the sealed arithmetic methods and representation-specific method predicates against core primitives. The same predicate syntax works for every width, so no per-width attribute hook is needed. Anodized accepts qualified outer attributes but requires nested trait and implementation attributes to use `#[spec]`; its enclosing macro consumes them. Trait-level attributes enable method specifications rather than accepting predicates. Generic methods without access to representation width or bounds retain their prose promises and refine them in each implementation.

The full workspace lint wall, formatter settings, test-runner configuration, baseline pin, hook floor, and four routed guidance pages are adopted together. `AGENTS.md` records project bindings and reversals. `[lints]` inherits the workspace lint tables; the Rust cfg declaration additionally recognizes `anodized_panic`. Two expectations on `Integer` cover Anodized-generated lowercase qualifier constants and forwarding methods without inline attributes; the lint wall remains unchanged elsewhere. Other deliberate deltas: one root library instead of a crate-category tree; no compiler-plugin tools or dependencies; Clippy/rustfmt/Miri components only; the toolchain task changes the shared channel alone; no parser fixture exclusions; the complete formatter tool set is installed. GitHub CI exposes separate build, lint, test, and format contexts using immutable action revisions and the pinned toolchain.

No release is automated. `publish = false` remains set. Mutation counts and survivor classifications belong to the reviewed change, not a permanent score claimed by this page.
