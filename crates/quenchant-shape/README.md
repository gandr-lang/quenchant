# quenchant-shape

`no_std` representations for non-failure absence and nominal primitive boundaries. Anodized is the only direct dependency, pinned to an audited core-only revision with default features disabled. The crate is Git-sourced and not published to crates.io.

## Absence carries its reason

`shape::Maybe<Value, Reason>` is a distinct enum: `Present(Value)` or `Absent(Reason)`. Each absence site names its own closed reason enum. Failure belongs in `Result<Value, Failure>`; a computation that can both fail and be absent returns `Result<Maybe<Value, Reason>, Failure>`.

| Operation       | Present input                                          | Absent input                                                       |
| --------------- | ------------------------------------------------------ | ------------------------------------------------------------------ |
| `map`           | Calls the mapper once and returns its value            | Preserves the exact reason without calling the mapper              |
| `and_then`      | Returns the continuation's value or reason             | Preserves the existing reason without calling the continuation     |
| `absent_reason` | Returns `Absent(absence_query::ValuePresent::Present)` | Returns the borrowed reason, leaving the original usable           |
| `into_result`   | Returns `Ok(value)` without calling the handler        | Calls the explicit boundary handler and returns its concrete error |

The combinators move or borrow data without allocating or adding a `Clone` bound. Callback behavior belongs to the caller. `into_result` requires an error implementing `core::error::Error`: the boundary must genuinely treat absence as failure. There is no default reason, `Try` implementation, or implicit conversion to `Option` or `Result`.

`reason_enum!` declares one site module, its enum, and a sealed `Reason` trait implemented only for that enum. Use the concrete enum in public signatures; a generic site-local helper can use the sealed trait as its bound. The generic `Maybe` container itself does not enforce the per-site enum policy. A shared sealed trait would prevent downstream sites from defining reasons; an open global marker would permit unrelated implementations. Per-site sealing keeps this responsibility with the caller. Revisit this choice only if Rust can express downstream-owned closed enum bounds without a catch-all marker.

## Iterator is an external boundary

A crate-defined iteration API returns `Maybe`. Only the standard `Iterator::next` implementation lowers normal exhaustion to the `Option` required by that trait.

```rust
use quenchant_shape::shape::Maybe;

quenchant_shape::reason_enum! {
    /// Normal completion of this cursor.
    pub mod cursor {
        /// Why this cursor has no next item.
        #[derive(Debug, Eq, PartialEq)]
        pub enum Exhausted {
            /// Its item has been consumed.
            Exhausted,
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Ticket {
    Admission,
}

#[repr(transparent)]
struct Cursor {
    pending: Maybe<Ticket, cursor::Exhausted>,
}

impl Cursor {
    // Specification: provides Exhausted when the single item has been consumed.
    fn next_value(&mut self) -> Maybe<Ticket, cursor::Exhausted> {
        core::mem::replace(
            &mut self.pending,
            Maybe::Absent(cursor::Exhausted::Exhausted),
        )
    }
}

impl Iterator for Cursor {
    type Item = Ticket;

    fn next(&mut self) -> Option<Self::Item> {
        // Standard-trait boundary: Option exists only in this adapter.
        match self.next_value() {
            Maybe::Present(ticket) => Some(ticket),
            Maybe::Absent(cursor::Exhausted::Exhausted) => None,
        }
    }
}

let mut tickets = Cursor { pending: Maybe::Present(Ticket::Admission) };
assert_eq!(tickets.next(), Some(Ticket::Admission));
assert_eq!(tickets.next_value(), Maybe::Absent(cursor::Exhausted::Exhausted));
assert_eq!(tickets.next(), None);
```

## Nominal wrappers and operators

`nominal_type!` declares a one-field `#[repr(transparent)]` type with a private representation and caller-selected derives. Constructors, validation, conversions, and primitive access remain the caller's responsibility. The macro adds no `Default`, `Deref`, or conversion implementation.

`delegate_ops!` emits a standard operator implementation in the wrapper's owning module. Its three forms are `binary`, `unary`, and `assign`. The selected safe operation receives the inner values, or a mutable left reference for assignment; value results are wrapped again. The macro adds no arithmetic policy, allocation, cloning, or unsafe block.

A consumer using `quenchant-arith` can select permanently strict arithmetic:

```rust
use quenchant_arith::arith::{self, Int};

quenchant_shape::nominal_type! {
    /// A count whose producer guarantees that sums fit.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Count(Int<usize>);
}

quenchant_shape::delegate_ops!(binary Count, Add::add => arith::strict_add);

assert_eq!(Count(Int::from(7_usize)) + Count(Int::from(2_usize)), Count(Int::from(9_usize)));
```

Scaffolding lives here; `quenchant-arith` retains its minimal arithmetic wrapper. Both crates use Anodized for executable specifications; neither depends on the other. Consumers choose their operation explicitly. A safe operator trait cannot carry an unchecked precondition, so it must not delegate to an unsafe fast entry point. Revisit the split only if shared scaffolding can preserve both crates' representation and arithmetic boundaries.

## Consumption and source copies

Private, unpublished workspaces use `https://github.com/silvanshade/quenchant` as a Git dependency, set Cargo's `rev` to an audited full commit SHA, disable defaults, and retain `Cargo.lock`. `cargo add quenchant-shape --git https://github.com/silvanshade/quenchant --no-default-features` starts integration; pin its moving source selection before landing the consumer change.

`delegate_ops!` emits Anodized attributes in the consuming crate. Consumers using that macro also declare `anodized` directly, pinned to `981bc892c979fd59c46aaaa2dc3075b8184f035f` from `https://github.com/silvanshade/anodized` with `default-features = false`.

A published package cannot depend on this unpublished Git package. Copy `src/shape.rs` with `src/tests.rs` beside it, and import the copy with `#[path = "vendor/quenchant_shape/shape.rs"] pub mod shape;`. The exported macros live at the consuming crate's root, so reserve their names. The consumer owns its package and license metadata and declares Anodized with defaults disabled. Publishing a source copy requires an audited core-only Anodized release available from a registry; the pinned Git fork does not satisfy Cargo's publication boundary.

Both copied files carry a header naming the canonical repository, the full source commit, and the copied path. For example, the implementation revision is `c00095b8efa502a1640b333947c16df69344c6bf`. After adding the headers, record SHA-256 fingerprints and check them in the consumer's gate with `sha256sum --check`. Update revision, source, and fingerprints together. A changed copy is a fork until reconciled with the canonical source. Run the copied module's witnesses in the consumer as well as its own integration checks.

## Verification and configuration

`mise run check` builds, runs the full lint wall, loads the external Dylint policy over every target, checks private-item rustdoc, and runs `mise run test` both normally and with graph-wide `RUSTFLAGS='--cfg anodized_panic'`. Each lane runs the six transition/wrapper tests in debug and release plus positive and compile-fail doctests. The enforcing lane adds a false-postcondition sentinel; the baseline hash and conflict-marker gates join the same fan-out. `mise run treefmt:check` checks formatting. Hosted CI exposes build, lint, test, test-anodized, and format contexts with pinned actions and toolchains.

Executable predicates check observable variant transitions. Move-only payload equality, callback history, caller-defined absence meaning, and declaration-level guarantees remain explicit prose obligations. `absent_reason` remains a const query without `#[spec]`: the pinned macro calls non-const `eval_once`, which rustc rejects with E0015 in a const function.

`mise run mutants:campaign` is a separate, explicitly requested experiment, outside normal gates. Its caught-inventory guard refuses an empty measurement. A passing test suite is not a mutation-adequacy result; execution status, viable counts, and classifications belong in the reviewed change rather than a permanent score here.

`mise run verus:derive` is the other separate experiment, also outside normal gates. It relocates each declared specification into a Verus proof input — the nominal declarations, the attribute's `captures` bindings and `ensures` body, and the function's own signature and body — and checks each one with the verifier named by `VERUS_BIN`; no predicate is authored in the adapter. The run names every specification-bearing construct in the library and its tests, including the ones it does not carry, and prints one verdict each. The sealed-bound absence constructor verifies. The three `Maybe` combinators do not: each calls an unspecified `FnOnce`, and the verifier requires a precondition for that call which no declared clause states. A proof result is a reviewed measurement, not a permanent score here.

The complete lint wall is inherited, with `anodized_panic` declared for enforcing builds. The external `quenchant` Dylint policy is loaded from a pinned Git revision, so the wall also carries `specification_present`: every function and method this crate authors states its specification or marks it trivial. Project deltas elsewhere are one root library, a channel-only toolchain bump beside a hand-moved plugin revision, no parser-fixture exclusions, and the shared hook floor. The four routed guidance pages and pinned baseline are adopted together; `AGENTS.md` records their project bindings. Publication stays manual with `publish = false`; no tag or release is automated.
