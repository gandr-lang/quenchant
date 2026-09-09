//! Refuse private material in tracked content, messages, and commit identities.

#![expect(
    clippy::std_instead_of_core,
    reason = "core::io is unstable on the supported compiler; filesystem errors use std"
)]
use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use quenchant_gates::GateError;
use quenchant_gates::semantic::ErrorMessage;
use quenchant_gates::semantic::SourceText;
use quenchant_shape::shape::Maybe;

use super::Passed;

/// The specific refused content class, without reproducing sensitive bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Material
{
    /// A user-specific filesystem path.
    HomePath,
    /// A nonpublic hostname.
    PrivateHost,
    /// An RFC 1918 address shape.
    PrivateAddress,
    /// An internal tool-resource URI.
    InternalUri,
    /// An access token or credential-bearing URL.
    Credential,
    /// A private-key delimiter.
    PrivateKey,
    /// A session token embedded in tracked content.
    SessionToken,
    /// A plaintext session identity or identifying trailer.
    SessionIdentity,
}

quenchant_shape::reason_enum! {
    /// Evidence that repository publication is refused.
    pub mod refusal {
        /// The first addressed public-boundary violation.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum Refused {
            /// A tracked control directory is outside the public surface.
            ControlDirectory(std::path::PathBuf),
            /// Refused material appears at an addressed source.
            Material {
                /// File and line, or commit-message line.
                location: String,
                /// The content class; sensitive values are not echoed.
                kind: super::Material,
            },
            /// A protected contributor identity exposes a private email.
            PrivateEmail(String),
        }
    }
}

quenchant_shape::reason_enum! {
    /// Evidence that a tracked tree contains an unresolved merge.
    pub mod conflict {
        /// The addressed unresolved marker.
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub enum Refused {
            /// A seven-character Git marker starts this line.
            Marker(String),
        }
    }
}

/// Compile all boundary recognizers once for a repository scan.
///
/// # Specification
/// - ensures: each recognizer carries the material class it refuses.
/// - fails: an invalid built-in expression returns `GateError::Parse`.
/// - panics: none.
///
/// # Errors
/// `GateError::Parse` identifies a malformed built-in expression.
///
/// # Adequacy
/// - hypothesis: L3 directed material classes and near misses distinguish
///   missing or widened recognizers.
/// - witness: `repository::public_boundary::tests::material_classes_and_near_misses`
fn patterns() -> Result<Vec<(Material, regex::Regex)>, GateError>
{
    let expressions = [
        (
            Material::HomePath,
            r#"/(Users|home)/[^/[:space:]\"<>]+/|[A-Za-z]:\\Users\\[^\\[:space:]\"<>]+\\"#,
        ),
        (
            Material::PrivateHost,
            r"(^|[^[:alnum:]_.-])((local[h]ost)|([[:alnum:]-]+\.)+(local|internal))([^[:alnum:]_.-]|$)",
        ),
        (
            Material::PrivateAddress,
            r"(^|[^0-9])(10\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}|192\.168\.[0-9]{1,3}\.[0-9]{1,3}|172\.(1[6-9]|2[0-9]|3[01])\.[0-9]{1,3}\.[0-9]{1,3})([^0-9]|$)",
        ),
        (
            Material::InternalUri,
            r"(skill|agent|history|artifact|local|omp)://",
        ),
        (
            Material::Credential,
            r"gh[pousr]_[A-Za-z0-9_]{20,}|AKIA[0-9A-Z]{16}|https://[^/[:space:]]+:[^/@[:space:]]+@",
        ),
        (
            Material::PrivateKey,
            r"^-----BEGIN ([A-Z0-9 ]+ )?PRIVATE KEY-----$",
        ),
        (Material::SessionToken, r"(?-u:\b)1\.[A-Za-z0-9_-]{60,}"),
        (
            Material::SessionIdentity,
            r"^[[:space:]]*([A-Za-z][A-Za-z0-9]*-Session:|Session:[[:space:]]+[^1[:space:]])",
        ),
    ];
    expressions
        .into_iter()
        .map(|(kind, expression)| {
            let expression = regex::Regex::new(expression).map_err(|error| {
                GateError::parse(
                    Path::new("public-boundary patterns"),
                    ErrorMessage(&error.to_string()),
                )
            })?;
            Ok((kind, expression))
        })
        .collect()
}

/// Distinguish tracked content from commit messages' permitted opaque
/// provenance.
#[derive(Clone, Copy)]
enum Surface
{
    /// Tracked files never carry session tokens.
    Tree,
    /// Opaque commit provenance is intentionally public.
    Message,
}

/// Locate refused material without including it in the diagnostic.
///
/// # Specification
/// - ensures: all lines are searched against every applicable material class.
/// - provides: `refusal::Refused::Material` with the address and class; commit
///   messages admit opaque tokens.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 classes, near misses, and provenance-surface separation
///   distinguish skipped and overbroad matches.
/// - witness: `repository::public_boundary::tests::material_classes_and_near_misses`
fn scan(
    text: SourceText<'_>,
    address: SourceText<'_>,
    surface: Surface,
    patterns: &[(Material, regex::Regex)],
) -> Maybe<Passed, refusal::Refused>
{
    let address = address.0;
    for (line, text) in text.0.lines().enumerate() {
        for &(kind, ref pattern) in patterns {
            if kind == Material::SessionToken && matches!(surface, Surface::Message) {
                continue;
            }
            if pattern.is_match(text) {
                return Maybe::Absent(refusal::Refused::Material {
                    location: format!("{address}:{}", line.saturating_add(1)),
                    kind,
                });
            }
        }
    }
    Maybe::Present(Passed)
}

/// Enforce the public boundary against the selected Git repository.
///
/// # Specification
/// - ensures: acceptance covers tracked working-tree text, all reachable commit
///   messages, and protected contributor identities.
/// - provides: sealed `refusal::Refused` evidence for control directories,
///   addressed material, or private email.
/// - fails: Git, file access, and decoding failures remain `GateError`; deleted
///   working-tree files are not scanned.
/// - panics: none.
///
/// # Errors
/// `GateError::Tool`, `GateError::Io`, and `GateError::Parse` prevent an
/// unmeasured pass.
///
/// # Adequacy
/// - hypothesis: L3 fixture trees, history-only residue, and private
///   contributor email separate the three observable surfaces.
/// - witness: `repository::public_boundary::tests::tracked_tree_and_history_are_checked`
/// - witness: `repository::public_boundary::tests::private_email_is_refused`
pub fn check(root: &Path) -> Result<Maybe<Passed, refusal::Refused>, GateError>
{
    let patterns = patterns()?;
    for path in super::tracked_paths(root)? {
        if path.components().any(|part| matches!(part, std::path::Component::Normal(name) if name == ".agents" || name == ".claude" || name == ".omp")) {
            return Ok(Maybe::Absent(refusal::Refused::ControlDirectory(path)));
        }
        let absolute = root.join(&path);
        let text = match std::fs::read_to_string(&absolute) {
            | Ok(text) => text,
            | Err(error) if error.kind() == ErrorKind::NotFound => continue,
            | Err(error) if error.kind() == ErrorKind::InvalidData => continue,
            | Err(error) => return Err(GateError::io(&path, ErrorMessage(&error.to_string()))),
        };
        if let Maybe::Absent(reason) = scan(
            SourceText(&text),
            SourceText(&path.display().to_string()),
            Surface::Tree,
            &patterns,
        ) {
            return Ok(Maybe::Absent(reason));
        }
    }
    let messages =
        super::output(
            Command::new("git")
                .current_dir(root)
                .args(["log", "--format=%B", "--all"]),
        )?;
    if let Maybe::Absent(reason) = scan(
        SourceText(&messages),
        SourceText("commit messages"),
        Surface::Message,
        &patterns,
    ) {
        return Ok(Maybe::Absent(reason));
    }
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .args(["log", "--format=%an%n%ae%n%cn%n%ce"]);
    // GitHub's synthetic merge author is not a contributor; inspect both real
    // parents.
    let pull_request = std::env::var("GITHUB_EVENT_NAME").is_ok_and(|name| name == "pull_request");
    let merge_parent = if pull_request {
        let output = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "--verify", "HEAD^2"])
            .output()
            .map_err(|error| GateError::io(root, ErrorMessage(&error.to_string())))?;
        output.status.success()
    }
    else {
        false
    };
    if merge_parent {
        command.args(["HEAD^1", "HEAD^2"]);
    }
    else {
        command.arg("--all");
    }
    let identities = super::output(&mut command)?;
    let mut lines = identities.lines();
    while let (Some(name), Some(email)) = (lines.next(), lines.next()) {
        if matches!(name, "agent-shade" | "silvanshade")
            && !email.ends_with("@users.noreply.github.com")
        {
            return Ok(Maybe::Absent(refusal::Refused::PrivateEmail(name.into())));
        }
    }
    Ok(Maybe::Present(Passed))
}

/// Refuse exact Git conflict markers while admitting decorative longer runs.
///
/// # Specification
/// - ensures: acceptance excludes seven marker characters followed by
///   whitespace or end-of-line in tracked text.
/// - provides: `conflict::Refused::Marker` identifies a violating file and
///   line.
/// - fails: inventory and readable-text access failures return `GateError`.
/// - panics: none.
///
/// # Errors
/// Git and file errors remain distinct from conflict evidence.
///
/// # Adequacy
/// - hypothesis: L3 exact-seven, longer-decoration, and suffix boundaries
///   distinguish widened or weakened recognition.
/// - witness: `repository::public_boundary::tests::conflict_marker_boundaries`
pub fn conflicts(root: &Path) -> Result<Maybe<Passed, conflict::Refused>, GateError>
{
    for path in super::tracked_paths(root)? {
        let text = match std::fs::read_to_string(root.join(&path)) {
            | Ok(text) => text,
            | Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::InvalidData) =>
            {
                continue;
            },
            | Err(error) => return Err(GateError::io(&path, ErrorMessage(&error.to_string()))),
        };
        for (line, text) in text.lines().enumerate() {
            if ["<<<<<<<", "=======", ">>>>>>>", "|||||||"]
                .iter()
                .any(|marker| {
                    text.strip_prefix(marker).is_some_and(|suffix| {
                        suffix.is_empty() || suffix.starts_with(char::is_whitespace)
                    })
                })
            {
                return Ok(Maybe::Absent(conflict::Refused::Marker(format!(
                    "{}:{}",
                    path.display(),
                    line.saturating_add(1)
                ))));
            }
        }
    }
    Ok(Maybe::Present(Passed))
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::repository::tests::Fixture;

    /// Create a repository with a safe, real initial commit.
    ///
    /// # Specification
    /// trivial.
    fn repository() -> Fixture
    {
        let fixture = Fixture::new();
        super::super::output(
            Command::new("git")
                .current_dir(&fixture.0)
                .args(["init", "-q"]),
        )
        .unwrap();
        commit(
            &fixture.0,
            String::from("initial"),
            String::from("agent-shade@users.noreply.github.com"),
        );
        fixture
    }

    /// Commit fixture content without consulting user hooks or signing
    /// services.
    ///
    /// # Specification
    /// trivial.
    fn commit(
        root: &Path,
        message: String,
        email: String,
    )
    {
        super::super::output(
            Command::new("git")
                .current_dir(root)
                .args([
                    "-c",
                    "core.hooksPath=/dev/null",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--allow-empty",
                    "-qm",
                ])
                .arg(message)
                .env("GIT_AUTHOR_NAME", "agent-shade")
                .env("GIT_COMMITTER_NAME", "agent-shade")
                .env("GIT_AUTHOR_EMAIL", &email)
                .env("GIT_COMMITTER_EMAIL", email),
        )
        .unwrap();
    }

    #[test]
    fn material_classes_and_near_misses()
    {
        let patterns = patterns().unwrap();
        let cases = [
            (Material::HomePath, ["/Us", "ers/example/work/"].concat()),
            (Material::HomePath, [r"C:\Us", r"ers\example\work"].concat()),
            (Material::PrivateHost, ["local", "host"].concat()),
            (Material::PrivateHost, ["machine.", "internal"].concat()),
            (Material::PrivateAddress, ["192.", "168.1.2"].concat()),
            (Material::InternalUri, ["artifact", "://fixture"].concat()),
            (Material::Credential, format!("ghp_{}", "x".repeat(20))),
            (
                Material::PrivateKey,
                ["-----BEGIN ", "PRIVATE KEY-----"].concat(),
            ),
            (Material::SessionToken, format!("1.{}", "x".repeat(60))),
            (
                Material::SessionIdentity,
                ["Session:", " plaintext"].concat(),
            ),
        ];
        for (kind, text) in cases {
            assert_eq!(
                scan(
                    SourceText(&text),
                    SourceText("fixture"),
                    Surface::Tree,
                    &patterns
                ),
                Maybe::Absent(refusal::Refused::Material {
                    location: "fixture:1".into(),
                    kind
                })
            );
        }
        let provenance = format!("Session: 1.{}", "x".repeat(60));
        assert_eq!(
            scan(
                SourceText(&provenance),
                SourceText("commit"),
                Surface::Message,
                &patterns
            ),
            Maybe::Present(Passed)
        );
        for text in [
            "https://example.com",
            "172.15.1.2",
            "172.32.1.2",
            "not-localhost.example.com",
        ] {
            assert_eq!(
                scan(
                    SourceText(text),
                    SourceText("fixture"),
                    Surface::Tree,
                    &patterns
                ),
                Maybe::Present(Passed)
            );
        }
    }

    #[test]
    fn tracked_tree_and_history_are_checked()
    {
        let fixture = repository();
        assert_eq!(check(&fixture.0).unwrap(), Maybe::Present(Passed));
        let path = fixture.0.join("address with spaces.txt");
        std::fs::write(&path, ["machine.", "local"].concat()).unwrap();
        super::super::output(
            Command::new("git")
                .current_dir(&fixture.0)
                .args(["add", "."]),
        )
        .unwrap();
        assert_eq!(
            check(&fixture.0).unwrap(),
            Maybe::Absent(refusal::Refused::Material {
                location: "address with spaces.txt:1".into(),
                kind: Material::PrivateHost
            })
        );
        std::fs::write(&path, "safe").unwrap();
        commit(
            &fixture.0,
            ["machine.", "local"].concat(),
            String::from("agent-shade@users.noreply.github.com"),
        );
        assert_eq!(
            check(&fixture.0).unwrap(),
            Maybe::Absent(refusal::Refused::Material {
                location: "commit messages:1".into(),
                kind: Material::PrivateHost
            })
        );
    }

    #[test]
    fn private_email_is_refused()
    {
        let fixture = repository();
        commit(
            &fixture.0,
            "private email".into(),
            "contributor@example.com".into(),
        );
        assert_eq!(
            check(&fixture.0).unwrap(),
            Maybe::Absent(refusal::Refused::PrivateEmail("agent-shade".into()))
        );
    }

    #[test]
    fn conflict_marker_boundaries()
    {
        let fixture = repository();
        let path = fixture.0.join("merge.txt");
        std::fs::write(&path, "safe").unwrap();
        super::super::output(
            Command::new("git")
                .current_dir(&fixture.0)
                .args(["add", "."]),
        )
        .unwrap();
        for marker in ["<<<<<<<", "=======", ">>>>>>>", "|||||||"] {
            for suffix in ["", " HEAD", "\tlabel"] {
                std::fs::write(&path, format!("{marker}{suffix}")).unwrap();
                assert_eq!(
                    conflicts(&fixture.0).unwrap(),
                    Maybe::Absent(conflict::Refused::Marker("merge.txt:1".into()))
                );
            }
            std::fs::write(&path, format!("{marker}x")).unwrap();
            assert_eq!(conflicts(&fixture.0).unwrap(), Maybe::Present(Passed));
        }
        std::fs::write(&path, "========").unwrap();
        assert_eq!(conflicts(&fixture.0).unwrap(), Maybe::Present(Passed));
    }
}
