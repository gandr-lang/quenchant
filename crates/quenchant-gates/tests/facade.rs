//! A standalone `no_std` consumer resolves only the facade package and
//! exercises stripped, compile-only, and enforcing interpretations.

#[test]
fn standalone_facade_preserves_const_values_and_enforces_predicates()
{
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("facade-consumer-{}-{serial}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let facade = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../quenchant-anodized")
        .canonicalize()
        .unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        format!(
            r#"[package]
name = "facade-consumer"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
quenchant-anodized = {{ path = {facade:?}, default-features = false }}
[lints.rust]
unexpected_cfgs = {{ level = "deny", check-cfg = ['cfg(anodized_panic)', 'cfg(anodized_print)'] }}
"#
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("src/lib.rs"),
        r#"#![cfg_attr(not(anodized_print), no_std)]
use anodized::spec;

/// A refinement independent of the optional logic layer.
#[derive(Clone, Copy)]
#[spec(maintains: self.0 > 0)]
pub struct Positive(pub u8);

/// A const expression remains evaluable under both interpretations.
#[cfg_attr(not(anodized_print), spec(requires: value > 0, ensures: |output| output > 0))]
pub const fn checked(value: u8) -> u8 { value }

/// Deliberately violate a postcondition without any other panic source.
#[spec(ensures: false)]
pub fn violated() {}

#[cfg(any(test, anodized_print))]
extern crate std;

#[cfg(test)]
mod tests {
    use super::{Positive, checked, violated};

    #[test]
    fn const_value_and_type_refinement() {
        const VALUE: u8 = checked(7);
        assert_eq!(VALUE, 7);
        assert_eq!(checked(9), 9);
        assert_eq!(Positive(11).0, 11);
        #[cfg(any(anodized_panic, anodized_print))]
        {
            use anodized::types::Spec;
            assert!(Positive(1).predicate());
            assert!(!Positive(0).predicate());
        }
    }

    #[test]
    fn violations_follow_selected_mode() {
        for (outcome, diagnostic) in [
            (std::panic::catch_unwind(|| checked(0)).map(|_| ()), "precondition failed"),
            (std::panic::catch_unwind(violated), "postcondition failed"),
        ] {
            #[cfg(anodized_panic)]
            {
                let failure = outcome.expect_err("the selected backend must enforce");
                let text = failure.downcast_ref::<&str>().copied()
                    .or_else(|| failure.downcast_ref::<std::string::String>().map(|text| text.as_str()))
                    .expect("enforcement panic carries text");
                assert!(text.contains(diagnostic), "wrong failure: {text}");
            }
            #[cfg(not(anodized_panic))]
            assert!(outcome.is_ok(), "unenforced {diagnostic} must preserve the body");
        }
    }
}
"#,
    )
    .unwrap();

    for (features, flags, admitted) in [
        ("", "", true),
        ("quenchant-anodized/anodized", "", true),
        ("quenchant-anodized/anodized", "--cfg anodized_panic", true),
        ("quenchant-anodized/anodized", "--cfg anodized_print", true),
        ("", "--cfg anodized_panic", false),
        ("", "--cfg anodized_print", false),
    ] {
        let mut command = std::process::Command::new(env!("CARGO"));
        command
            .current_dir(&root)
            .args(["test", "--offline", "--no-default-features"])
            .env("CARGO_TARGET_DIR", root.join("target"))
            .env("RUSTFLAGS", format!("-D warnings {flags}"))
            .env("RUSTDOCFLAGS", format!("-D warnings {flags}"))
            .env_remove("CARGO_ENCODED_RUSTDOCFLAGS")
            .env_remove("CARGO_BUILD_RUSTDOCFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("NEXTEST_PROFILE");
        if !features.is_empty() {
            command.args(["--features", features]);
        }
        command.args(["--", "--nocapture"]);
        let output = command.output().unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.success(),
            admitted,
            "consumer features={features:?} flags={flags:?}:\n{}\n{stderr}",
            String::from_utf8_lossy(&output.stdout),
        );
        if !admitted {
            assert!(stderr.contains("ANODIZED_BACKEND_DISABLED"), "{stderr}");
        }
        else if flags == "--cfg anodized_print" {
            assert!(stderr.contains("postcondition failed"), "{stderr}");
        }
    }

    let graph = std::process::Command::new(env!("CARGO"))
        .current_dir(&root)
        .args([
            "tree",
            "--offline",
            "--no-default-features",
            "--prefix",
            "none",
        ])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        graph.status.success(),
        "{}",
        String::from_utf8_lossy(&graph.stderr)
    );
    let graph = String::from_utf8(graph.stdout).unwrap();
    assert!(
        graph.lines().all(|line| !matches!(
            line.split_whitespace().next(),
            Some("anodized" | "anodized-core" | "anodized-macros")
        )),
        "feature-off dependency graph contains the backend:
{graph}",
    );

    std::fs::create_dir_all(root.join("src/bin")).unwrap();
    std::fs::write(
        root.join("src/bin/references.rs"),
        r#"use anodized::spec;
use core::cmp::Ordering as PredicateOrder;
use core::sync::atomic::{AtomicUsize, Ordering};

static CAPTURES: AtomicUsize = AtomicUsize::new(0);
static PREDICATES: AtomicUsize = AtomicUsize::new(0);

fn predicate_only(value: u8) -> bool {
    PREDICATES.fetch_add(1, Ordering::Relaxed);
    value > 0
}

fn capture(value: u8) -> u8 {
    CAPTURES.fetch_add(1, Ordering::Relaxed);
    value
}

#[spec(
    requires: predicate_only(value),
    captures: before = capture(value),
    ensures: |output| output.cmp(&before) == PredicateOrder::Equal,
)]
fn retained(value: u8) -> u8 { value }

#[spec(requires: predicate_only(state))]
fn parameter_only(state: u8) {}

const fn const_predicate_only(value: u8) -> bool { value > 0 }

#[spec(requires: const_predicate_only(value), ensures: |output| const_predicate_only(output))]
const fn retained_const(value: u8) -> u8 { value }

fn main() {
    const VALUE: u8 = retained_const(7);
    assert_eq!(VALUE, 7);
    assert_eq!(retained_const(9), 9);
    assert_eq!(retained(11), 11);
    parameter_only(13);
    assert_eq!(CAPTURES.load(Ordering::Relaxed), 1);
    assert_eq!(PREDICATES.load(Ordering::Relaxed), if cfg!(anodized_panic) { 2 } else { 0 });
}
"#,
    )
    .unwrap();
    for flags in ["", "--cfg anodized_panic"] {
        let output = std::process::Command::new(env!("CARGO"))
            .current_dir(&root)
            .args([
                "run",
                "--offline",
                "--no-default-features",
                "--bin",
                "references",
                "--features",
                "quenchant-anodized/anodized",
            ])
            .env("CARGO_TARGET_DIR", root.join("target"))
            .env("RUSTFLAGS", format!("-D warnings {flags}"))
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "predicate references flags={flags:?}:
{}
{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    std::fs::write(
        root.join("src/bin/helpers.rs"),
        r#"#![feature(proc_macro_hygiene)]
use anodized::{spec, spec_helper};
use core::sync::atomic::{AtomicUsize, Ordering};

#[spec_helper]
use core::cmp::Ordering as PredicateOrder;

static INITIALIZERS: AtomicUsize = AtomicUsize::new(0);
static PREDICATES: AtomicUsize = AtomicUsize::new(0);

#[spec_helper]
#[derive(Clone, Copy, PartialEq)]
enum ConstEquality { Equal, Different }

#[spec_helper]
const fn const_eq(left: u8, right: u8) -> ConstEquality {
    if left == right { ConstEquality::Equal } else { ConstEquality::Different }
}

#[spec_helper]
fn positive(value: u8) -> bool {
    PREDICATES.fetch_add(1, Ordering::Relaxed);
    value.cmp(&0) == PredicateOrder::Greater
}

#[spec(requires: {
    #[spec_helper]
    let threshold = INITIALIZERS.fetch_add(1, Ordering::Relaxed);
    positive(value) && usize::from(value) > threshold
}, ensures: |output| const_eq(output, value) == ConstEquality::Equal)]
fn checked(value: u8) -> u8 { value }

fn local_binding() {
    #[spec_helper]
    const LIMIT: u8 = 9;
    #[spec(requires: value < LIMIT)]
    fn bounded(value: u8) -> u8 { value }
    assert_eq!(bounded(7), 7);
}

fn main() {
    assert_eq!(checked(7), 7);
    local_binding();
    let backend = std::env::args().nth(1).as_deref() == Some("backend");
    let enforcing = cfg!(any(anodized_panic, anodized_print));
    assert_eq!(INITIALIZERS.load(Ordering::Relaxed), usize::from(enforcing));
    assert_eq!(PREDICATES.load(Ordering::Relaxed), usize::from(enforcing));
    println!("spec_helper: backend={backend} enforcing={enforcing}");
}
"#,
    )
    .unwrap();
    for (features, flags) in [
        ("", ""),
        ("quenchant-anodized/anodized", ""),
        ("quenchant-anodized/anodized", "--cfg anodized_panic"),
        ("quenchant-anodized/anodized", "--cfg anodized_print"),
    ] {
        let mut command = std::process::Command::new(env!("CARGO"));
        command
            .current_dir(&root)
            .args([
                "run",
                "--offline",
                "--no-default-features",
                "--bin",
                "helpers",
            ])
            .env("CARGO_TARGET_DIR", root.join("target"))
            .env("RUSTFLAGS", format!("-D warnings {flags}"))
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET");
        if !features.is_empty() {
            command.args(["--features", features, "--", "backend"]);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "specification helpers features={features:?} flags={flags:?}:
{}
{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    std::fs::remove_dir_all(root).unwrap();
}
