import { execFileSync } from "node:child_process";

// Real trailer tokens. The conventional-commits parser treats ANY `word:` line
// as the footer start, so the stock footer-leading-blank rule misfires on
// wrapped prose; this closed list is what makes the replacement rule sound.
const TRAILER_TOKENS = [
  "BREAKING CHANGE",
  "BREAKING-CHANGE",
  "Acked-by",
  "Cc",
  "Closes",
  "Co-Authored-By",
  "Fixes",
  "Refs",
  "Reported-by",
  "Reviewed-by",
  // The provenance block's own two tokens. Without them the block's first line
  // reads as prose, so the leading-blank check fires on the line below it
  // rather than on the block, and an agent commit is refused for a defect it
  // does not have.
  "Role",
  "Session",
  "Signed-off-by",
  "Tested-by",
];
const TRAILER_LINE = new RegExp(`^(?:${TRAILER_TOKENS.join("|")}):[ \\t]`, "i");

const trailerLeadingBlank = (parsed) => {
  const raw = parsed.raw ?? [parsed.header, parsed.body, parsed.footer].filter(Boolean).join("\n");
  const lines = raw.split("\n");
  const first = lines.findIndex((line) => TRAILER_LINE.test(line.trimEnd()));
  if (first <= 0) return [true, ""];
  if ((lines[first - 1] ?? "").trim() === "") return [true, ""];
  return [
    false,
    `the trailer block must be preceded by a blank line; found "${(lines[first] ?? "").trim()}"`,
  ];
};

// Closed vocabulary. Grow deliberately; per-surface growth is the failure mode.
//
// delta: the scope list is per project. Every crate here shares the one
// `quenchant-` package prefix, so a category axis would carry exactly one value;
// the crate scopes are the crate names with that prefix dropped, and a scope
// survives a crate split because it names the concern rather than the
// directory. Infra scopes name the surfaces that carry no crate.
const SCOPES = [
  "agents",
  "arith",
  "ci",
  "config",
  "docs",
  "dylints",
  "fixtures",
  "gates",
  "github",
  "repo",
  "review",
  "shape",
];

// A harness-forensics trailer (`<harness>-Session:`) records which tool drove a
// commit. That is contributor-concern, never project-concern, and it outlives
// the session it points at, so it must never reach a published history. The
// pattern matches by shape rather than by a list of tool names, so a harness
// nobody here has heard of is refused on the same terms.
const HARNESS_TRAILER_LINE = /^[A-Za-z][A-Za-z0-9]*-Session:/i;

const noHarnessTrailer = (parsed) => {
  const raw = parsed.raw ?? [parsed.header, parsed.body, parsed.footer].filter(Boolean).join("\n");
  const offenders = raw.split("\n").filter((line) => HARNESS_TRAILER_LINE.test(line.trim()));
  if (offenders.length === 0) return [true, ""];
  return [
    false,
    `harness trailers are contributor-concern: ${offenders.map((line) => line.trim()).join(", ")}`,
  ];
};

// The commit's author. CI lints landed commits one at a time and names each
// in COMMITLINT_COMMIT, so the author is the one that commit records. The
// local hook lints a commit not yet made, so the author is the one git will
// record, resolved the way a prepare-commit-msg hook resolves it:
// `git var GIT_AUTHOR_IDENT` honours GIT_AUTHOR_* exported by rebase and
// cherry-pick, so a replayed owner commit stays exempt and a replayed agent
// commit stays bound. An explicitly named commit that git cannot resolve fails
// the lint rather than passing as a non-agent author.
const commitAuthor = () => {
  const commit = process.env.COMMITLINT_COMMIT;
  const args = commit ? ["show", "-s", "--format=%an <%ae>", commit] : ["var", "GIT_AUTHOR_IDENT"];
  try {
    return execFileSync("git", args, { encoding: "utf8" }).trim();
  } catch (error) {
    if (commit)
      throw new Error(`COMMITLINT_COMMIT=${commit}: cannot resolve its author`, { cause: error });
    return "";
  }
};

const OWNER_COAUTHOR = "Co-authored-by: silvanshade <silvanshade@users.noreply.github.com>";

// An agent commit carries exactly one line beyond its prose: the owner
// co-author line, crediting the coordinating owner. The Role, Session, and
// Assisted-by block that once rode beside it is retired: a session token is
// contributor-concern that outlives the session it points at, and the role is
// the assignment's, not the commit's. A commit that still carries one of those
// lines is refused so the retired form cannot creep back through a template.
// The form of the owner line binds whatever the author; its presence binds
// agent authors only, resolved the way a prepare-commit-msg hook resolves it.
const RETIRED_PROVENANCE_LINE = /^(?:Role|Session|Assisted-by):/i;

const ownerCoauthorRequired = (parsed) => {
  const raw = parsed.raw ?? "";
  // Resolved first, so an unresolvable COMMITLINT_COMMIT fails even when the
  // message is otherwise complete.
  const author = commitAuthor();
  const lines = raw.split("\n").map((line) => line.trimEnd());

  const retired = lines.find((line) => RETIRED_PROVENANCE_LINE.test(line));
  if (retired)
    return [false, `the provenance block is retired; drop "${retired}" and keep ${OWNER_COAUTHOR}`];

  const owners = lines.filter((line) => /^Co-authored-by:[ \t]+silvanshade/i.test(line));
  const bad = owners.find((line) => line !== OWNER_COAUTHOR);
  if (bad) return [false, `malformed owner co-author line "${bad}": exact form required`];
  if (owners.length > 1)
    return [false, `exactly one owner co-author line per commit, found ${owners.length}`];

  if (!author.startsWith("agent-")) return [true, ""];
  if (owners.length === 0) return [false, `an agent commit carries ${OWNER_COAUTHOR}`];
  return [true, ""];
};

// Co-authored-by credits people, the owner and any other human alike; a line
// naming a known assistant identity is refused outright. Match full assistant
// names, service domains, or explicit bot markers. A word inside a human name
// or personal email does not identify an assistant.
const ASSISTANT_COAUTHOR =
  /^Co-authored-by:\s*(?:anthropic|claude(?: code)?|openai|chatgpt|codex|(?:github )?copilot|coderabbit(?:ai)?|gemini|cursor|llm)\s*(?:<[^<>]*>)?$|<[^<>@]+@(?:[^<>@]+\.)?(?:anthropic\.com|openai\.com|coderabbit\.ai)>|\[bot\]/i;

const noAssistantCoauthor = (parsed) => {
  const raw = parsed.raw ?? "";
  const assistant = raw
    .split("\n")
    .map((line) => line.trimEnd())
    .find((line) => /^Co-authored-by:/i.test(line) && ASSISTANT_COAUTHOR.test(line));
  if (assistant) return [false, `an assistant is never a co-author: "${assistant}"`];
  return [true, ""];
};

export default {
  extends: ["@commitlint/config-conventional"],
  plugins: [
    {
      rules: {
        "trailer-leading-blank": trailerLeadingBlank,
        "no-harness-trailer": noHarnessTrailer,
        "owner-coauthor-required": ownerCoauthorRequired,
        "no-assistant-coauthor": noAssistantCoauthor,
      },
    },
  ],
  rules: {
    "header-max-length": [2, "always", 72],
    "header-trim": [2, "always"],
    "subject-empty": [2, "never"],
    "subject-full-stop": [2, "never", "."],
    "body-leading-blank": [2, "always"],
    "body-max-line-length": [2, "always", 100],
    // Disabled: the conventional-commits parser reclassifies wrapped prose
    // bodies as footer whenever a line starts with `word:`; the custom
    // trailer-leading-blank rule above is the sound replacement.
    "footer-leading-blank": [0, "always"],
    "trailer-leading-blank": [2, "always"],
    "no-harness-trailer": [2, "always"],
    "owner-coauthor-required": [2, "always"],
    "no-assistant-coauthor": [2, "always"],
    // Stock conventional types plus config, for changes to the repository's
    // configuration surfaces (lint vocabularies, tool settings).
    "type-enum": [
      2,
      "always",
      [
        "build",
        "chore",
        "ci",
        "config",
        "docs",
        "feat",
        "fix",
        "perf",
        "refactor",
        "revert",
        "style",
        "test",
      ],
    ],
    "type-empty": [2, "never"],
    "scope-empty": [2, "never"],
    "scope-case": [2, "always", "lower-case"],
    "scope-enum": [2, "always", SCOPES],
  },
};
