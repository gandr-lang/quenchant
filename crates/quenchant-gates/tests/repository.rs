//! CLI refusal fixtures exercise the real repository commands, not mocked
//! instruments.

use std::path::PathBuf;
use std::process::Command;

/// Owned temporary repository, isolated from user Git hooks and signing
/// services.
#[repr(transparent)]
struct Fixture(PathBuf);

impl Drop for Fixture
{
    /// Remove only this test's owned fixture.
    ///
    /// # Specification
    /// trivial.
    fn drop(&mut self)
    {
        std::fs::remove_dir_all(&self.0).expect("remove fixture");
    }
}

#[test]
fn subcommands_refuse_broken_fixtures()
{
    let fixture = Fixture(
        std::env::temp_dir().join(format!("quenchant-cli-refusals-{}", std::process::id())),
    );
    std::fs::create_dir_all(fixture.0.join(".github/workflows")).expect("create fixture");
    std::fs::create_dir_all(fixture.0.join("src")).expect("create source directory");
    std::fs::write(fixture.0.join("src/lib.rs"), "").expect("create package source");
    std::fs::write(
        fixture.0.join("Cargo.toml"),
        r#"
[package]
name = "outside"
version = "0.0.1"
publish = false
[workspace]
[workspace.dependencies]
clippy_utils = { tag = "rust-1.98.0" }
dylint_linting = { version = "6.0.4" }
dylint_testing = { version = "6.0.3" }
"#,
    )
    .expect("write manifest");
    std::fs::write(
        fixture.0.join("mise.toml"),
        r#"
[tools]
"cargo:cargo-dylint" = "6.0.4"
"cargo:dylint-link" = "6.0.4"
"#,
    )
    .expect("write tools");
    let toolchain = "[toolchain]\nchannel = 'nightly-2026-07-30'\n";
    std::fs::write(fixture.0.join("rust-toolchain.toml"), toolchain).expect("write channel");
    std::fs::write(fixture.0.join("upstream.toml"), toolchain).expect("write upstream");
    std::fs::write(
        fixture.0.join(".github/workflows/ci.yml"),
        "steps: [{uses: unknown/action@v1}]\n",
    )
    .expect("write action");
    std::fs::write(
        fixture.0.join("private.txt"),
        ["machine.", "local"].concat(),
    )
    .expect("write boundary fixture");
    for arguments in [vec!["init", "-q"], vec!["add", "."], vec![
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "fixture",
    ]] {
        let output = Command::new("git")
            .current_dir(&fixture.0)
            .args(arguments)
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
            .output()
            .expect("run fixture Git");
        assert!(
            output.status.success(),
            "fixture Git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for (command, reason, options) in [
        ("public-boundary", "PrivateHost", Vec::new()),
        ("pins", "Drift", vec![
            "--upstream-toolchain",
            "upstream.toml",
        ]),
        ("action-pins", "Unlisted", Vec::new()),
        ("publish-allowlist", "Unclassified", Vec::new()),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_quenchant-gates"))
            .arg(command)
            .arg("--root")
            .arg(&fixture.0)
            .args(options)
            .env_remove("WORKSPACE_MANIFEST")
            .env_remove("TOOL_CONFIG")
            .env_remove("TOOLCHAIN_FILE")
            .output()
            .expect("run repository command");
        let diagnostic = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
        assert!(
            !output.status.success(),
            "broken {command} fixture must be refused"
        );
        assert!(
            diagnostic.contains("FAILED:") && diagnostic.contains(reason),
            "{command} needs its classified refusal, got {diagnostic}"
        );
        assert!(
            output.stdout.is_empty(),
            "a refusal must not report acceptance"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_quenchant-gates"))
        .args(["pins", "--root"])
        .output()
        .expect("run missing argument");
    assert!(
        !output.status.success(),
        "missing scope value must not run the check"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_quenchant-gates"))
        .args(["action-pins", "--root"])
        .arg(&fixture.0)
        .args(["--root", "."])
        .output()
        .expect("run duplicate argument");
    assert!(
        !output.status.success(),
        "duplicate scope must not select an unintended tree"
    );
}
