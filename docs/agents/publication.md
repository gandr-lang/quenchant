# Publication

Public-facing files and messages MUST be independently usable. Keep workstation details, private coordination material, private source links, session residue, and dispatch routing out of them. A private source may inform an explanation; the public explanation carries its needed reasoning and public citations rather than depending on access to that source.

`check:public-boundary` checks tracked content and repository metadata for known leaks. It is a floor, not an exhaustive proof that a document is safe to publish. Trace examples, generated outputs, and new references through the same boundary.

## Provenance

Commit validation is defined by `commitlint.config.mjs`: an accepted type and scope, a nonempty purpose, and the required provenance trailers. No project hook supplies the trailer block automatically. The author types the externally supplied role, opaque session token, and owner co-author fields; credentials and plaintext session identities never enter the message.

Repository discussions use the corresponding role/token frontmatter. NEVER add a second identity trailer or place routing aliases in the artifact. Commit prose explains the change and its constraint; it is not a session transcript.

## Package preparation

The manifest gate checks the exact package eligibility boundary and keeps the fixture macro package unpublished. A package's eligibility does not establish that it can resolve unpublished siblings, that it has been uploaded, or that every compiler-internal dependency is registry-ready.

Cargo can prepare the interdependent library/macro family together. Its temporary packaging registry permits dependency-source checks and verification without a real upload. When reporting a dry run, distinguish packaging, verification, sibling-resolution limitations, and the explicit aborted upload. `--no-verify` supplies packaging evidence only.

One license covers every package, and its text sits at the repository root as the single copy; a packaged crate carries the identifier its manifest inherits. Literal source titles, external API identifiers, historical filenames, and legal license wording retain their exact spelling; they are not alternative project terminology.

Actual package publication and repository visibility changes remain manual owner decisions. The CI and local gate graph contain no upload step.
