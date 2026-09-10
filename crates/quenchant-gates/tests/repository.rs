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

#[test]
fn witness_inventory_uses_consumer_selection_over_shadowed_plugins()
{
    let fixture = Fixture(
        std::env::temp_dir().join(format!("quenchant-runner-selection-{}", std::process::id())),
    );
    let consumer = fixture.0.join("consumer");
    let launch = fixture.0.join("launch");
    let cargo_home = fixture.0.join("cargo-home");
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::create_dir_all(consumer.join("src")).expect("create consumer");
    std::fs::create_dir_all(&launch).expect("create unrelated launch directory");
    std::fs::create_dir_all(cargo_home.join("bin")).expect("create adversarial Cargo home");
    let selected = Command::new("mise")
        .args(["which", "cargo-nextest", "--version"])
        .current_dir(&source_root)
        .output()
        .expect("resolve test runner selection");
    assert!(
        selected.status.success(),
        "{}",
        String::from_utf8_lossy(&selected.stderr)
    );
    let selected = String::from_utf8(selected.stdout).expect("selected version is text");
    std::fs::write(
        consumer.join("mise.toml"),
        format!(
            r#"[tools]
"github:nextest-rs/nextest" = {:?}
"#,
            selected.trim()
        ),
    )
    .expect("write consumer runner selection");
    std::fs::write(
        launch.join("mise.toml"),
        r#"[tools]
"github:nextest-rs/nextest" = "cargo-nextest-0.0.0-unavailable"
"#,
    )
    .expect("write deliberately unusable launch selection");
    std::fs::copy(
        source_root.join("rust-toolchain.toml"),
        consumer.join("rust-toolchain.toml"),
    )
    .expect("retain supported consumer compiler");
    std::fs::write(
        launch.join("rust-toolchain.toml"),
        r#"[toolchain]
channel = "nightly-1900-01-01"
"#,
    )
    .expect("write unrelated unavailable launch compiler");
    std::fs::write(
        consumer.join("Cargo.toml"),
        r#"[package]
name = "runner-consumer"
version = "0.0.0"
edition = "2024"
"#,
    )
    .expect("write consumer manifest");
    let source = r#"
/// One available test is an inventory witness, not an executed proof.
///
/// # Adequacy
/// - hypothesis: the declared test is present in this package's inventory.
/// - witness: `tests::present`
pub fn operation() {}

#[cfg(test)]
mod tests {
    #[test]
    fn present() {}
}
"#;
    std::fs::write(consumer.join("src/lib.rs"), source).expect("write consumer source");
    let poison = fixture.0.join("poison.rs");
    std::fs::write(
        &poison,
        r#"
fn main() {
    let marker = std::env::var_os("QUENCHANT_SHADOW_MARKER").expect("marker");
    std::fs::write(marker, b"unselected plugin executed").expect("write marker");
    std::process::exit(99);
}
"#,
    )
    .expect("write adversarial executable source");
    let executable = cargo_home
        .join("bin")
        .join(format!("cargo-nextest{}", std::env::consts::EXE_SUFFIX));
    let compiled = Command::new("rustc")
        .arg(&poison)
        .arg("-o")
        .arg(&executable)
        .current_dir(&source_root)
        .output()
        .expect("compile adversarial executable");
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let marker = fixture.0.join("shadow-ran");
    let path = std::env::var_os("PATH").expect("test has PATH");
    let path = std::env::join_paths(
        core::iter::once(cargo_home.join("bin")).chain(std::env::split_paths(&path)),
    )
    .expect("construct isolated adversarial PATH");
    let mut command = Command::new(env!("CARGO_BIN_EXE_quenchant-gates"));
    command
        .args(["witnesses", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .current_dir(&launch)
        .env("CARGO_HOME", &cargo_home)
        .env("CARGO", fixture.0.join("unselected-cargo"))
        .env("RUSTUP_TOOLCHAIN", "nightly-1900-01-02")
        .env("PATH", path)
        .env("QUENCHANT_SHADOW_MARKER", &marker)
        .env("MISE_TRUSTED_CONFIG_PATHS", &fixture.0);
    let output = command.output().expect("inventory actual consumer");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !marker.exists(),
        "inventory must never execute the unselected Cargo plugin"
    );

    std::fs::write(
        consumer.join("src/lib.rs"),
        source.replace("tests::present", "tests::missing"),
    )
    .expect("name an absent witness");
    let output = command.output().expect("reject actual missing witness");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("unresolved-witness"),
        "{}",
        String::from_utf8_lossy(&output.stdout),
    );
    assert!(
        !marker.exists(),
        "a failed witness must not change the selected instrument"
    );

    std::fs::write(
        consumer.join("src/lib.rs"),
        r#"compile_error!("invalid consumer source");"#,
    )
    .expect("force a real selected-runner compilation failure");
    let output = command
        .output()
        .expect("run selected inventory on invalid source");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(selected.trim()),
        "failed inventory must identify the selected executable: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        !marker.exists(),
        "an operational failure must not substitute a global plugin"
    );
}
