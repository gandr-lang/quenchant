//! Installed-command evidence for consumer selection and flag precedence.

#[test]
fn consumer_configuration_and_encoded_flags_select_the_state()
{
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("workflow-cfg-{}-{serial}", std::process::id()));
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        r#"[package]
name = "cfg-consumer"
version = "0.0.0"
edition = "2024"
[workspace]
"#,
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "").unwrap();
    std::fs::write(
        root.join(".cargo/config.toml"),
        r#"[build]
rustflags = ["--cfg", "anodized_print"]
"#,
    )
    .unwrap();

    // Launching elsewhere distinguishes manifest-selected configuration from
    // caller-local configuration; encoded flags must override both it and
    // RUSTFLAGS.
    for (encoded, expected, expected_code) in [
        (None, "enforcing", Some(0_i32)),
        (
            Some("--cfg\u{1f}anodized_discard_specs"),
            "discarded",
            Some(1_i32),
        ),
        (Some(""), "non-enforcing", Some(1_i32)),
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_quenchant-gates"));
        command
            .args(["anodized", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            .arg("--require-enforcing")
            .env("RUSTFLAGS", "--cfg anodized_panic")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET");
        if let Some(encoded) = encoded {
            command.env("CARGO_ENCODED_RUSTFLAGS", encoded);
        }
        let output = command.output().unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(output.status.code(), expected_code, "{stdout}{stderr}");
        assert_eq!(
            stdout.trim().rsplit(": ").next(),
            Some(expected),
            "{stdout}{stderr}"
        );
    }

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_quenchant-gates"))
        .args(["anodized", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET")
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0_i32), "{stdout}");
    assert_eq!(
        stdout.trim().rsplit(": ").next(),
        Some("print-only"),
        "{stdout}"
    );
    std::fs::remove_dir_all(root).unwrap();
}
