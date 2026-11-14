//! The lint library's build id: a hash of its manifest and its sources. A reach
//! record that one build of the library writes is read by that build alone.

use std::path::PathBuf;

/// The offset basis of 64-bit FNV-1a.
const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// The prime of 64-bit FNV-1a.
const PRIME: u64 = 0x0000_0100_0000_01b3;

/// Hash the manifest and each source into `QUENCHANT_DYLINTS_BUILD`.
///
/// # Specification
/// - ensures: the build id is the 64-bit FNV-1a hash of `Cargo.toml` and each
///   `.rs` file under `src`, in sorted path order, each as its path relative to
///   the package and then its bytes; Cargo runs the script again when the
///   manifest or a file under `src` changes.
/// - fails: a directory or a source that cannot be read stops the build, and
///   Cargo shows the error.
/// - panics: none.
/// - executable: none — the outcome is an instruction to Cargo.
///
/// # Errors
/// An `std::io::Error` of a directory listing or a file read.
///
/// # Adequacy
/// - hypothesis: L3 — the fixtures' dependency writes its reach record under
///   this build id and the fixtures read it back under the same id; a build id
///   that the record does not carry refuses the record by name.
/// - witness: `tests::ui_embedded_source`
fn main() -> std::io::Result<()>
{
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let mut pending = vec![root.join("src")];
    let mut files = vec![root.join("Cargo.toml")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            }
            else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut hash = OFFSET;
    for file in &files {
        let relative = file.strip_prefix(&root).unwrap_or(file);
        let bytes = std::fs::read(file)?;
        for &byte in relative.as_os_str().as_encoded_bytes().iter().chain(&bytes) {
            hash = (hash ^ u64::from(byte)).wrapping_mul(PRIME);
        }
    }
    println!("cargo::rerun-if-changed=src");
    println!("cargo::rerun-if-changed=Cargo.toml");
    println!("cargo::rustc-env=QUENCHANT_DYLINTS_BUILD={hash:016x}");
    Ok(())
}
