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
    std::fs::remove_dir_all(root).unwrap();
}
