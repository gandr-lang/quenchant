//! The hotpath verdict: a pull request whose hotpath check reports a
//! regression owes, in its body, the regression fixed or the growth accepted.
//!
//! **The report.** `--hotpath <report>` names the check's per-benchmark
//! deltas as JSON: an array of `{"benchmark", "before", "after",
//! "regressed"}`, the numbers as the check writes them. The block is owed
//! when any benchmark regressed.
//!
//! **The two shapes.** `hotpath: fixed — <benchmark> <before> → <after>`
//! says the regression is gone on the branch's head, in the check's own
//! numbers. `hotpath: accepted — <benchmark> exercises <feature>; <n>
//! allocations per <use>; <one sentence>` accepts the growth, with its cost
//! per use on an input that uses the feature and the cheaper shape that
//! lost. Both name a benchmark of the report. An `accepted` naming a
//! benchmark the report does not move is refused: growth on an input the
//! change does not reach is a defect to fix, never a cost to accept. The size
//! of a diff is never the justification; the cost per use is.
//!
//! **Reading it back.** The verdict is read from the body, or from the
//! previous text when no body is given, as the lint assessment is, so a
//! rewrite of the section keeps what the author wrote.

use std::path::Path;

use anodized::spec;
use quenchant_shape::shape::Maybe;

use crate::GateError;
use crate::semantic::DocText;
use crate::semantic::ErrorMessage;
use crate::semantic::InShape;
use crate::semantic::Unsettled;

/// The marker that closes the hotpath verdict's task line.
pub const HOTPATH_MARKER: &str = "<!-- ack hotpath-verdict -->";

/// The verdict a fresh task line holds, for the author to replace.
pub const HOTPATH_TEMPLATE: &str = "`hotpath: fixed — <benchmark> <before> → <after>` or \
                                    `hotpath: accepted — <benchmark> exercises <feature>; <n> \
                                    allocations per <use>; <one sentence>`";

/// One benchmark's delta in the check's report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Delta
{
    /// The benchmark's name: `gandr`.
    pub benchmark: String,
    /// Its number before the change, as the check writes it.
    pub before: String,
    /// Its number after the change.
    pub after: String,
    /// Whether the check reads it over its policy's threshold.
    pub regressed: Regressed,
}

/// Whether a benchmark's delta is over its policy's threshold.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Regressed
{
    /// Over the threshold: the block is owed.
    Yes,
    /// Within it.
    No,
}

/// The check's per-benchmark deltas.
#[repr(transparent)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Deltas(pub Vec<Delta>);

/// Read the check's report from its JSON text.
///
/// # Specification
/// - requires: nothing.
/// - ensures: one delta per array element, in order, each from its `benchmark`,
///   `before` and `after` strings and its `regressed` boolean.
/// - provides: the report the verdict is owed and checked against.
/// - fails: [`GateError::Parse`] naming `path` when the text is not an array of
///   such objects.
/// - panics: none.
/// - executable: none — the outcome is the parse itself.
///
/// # Errors
/// As `fails` lists.
///
/// # Adequacy
/// - hypothesis: L3 — a report of a regressed and an unmoved benchmark, and a
///   report missing a field; a lost element, a swapped number or an accepted
///   malformed report changes the obligation.
/// - witness: `gates::obligations::a_red_hotpath_check_owes_a_verdict`
#[inline]
pub fn read_report(
    text: DocText<'_>,
    path: &Path,
) -> Result<Deltas, GateError>
{
    let malformed = |what: &str| {
        GateError::parse(
            path,
            ErrorMessage::from(&format!("a hotpath report {what}")),
        )
    };
    let value: serde_json::Value = serde_json::from_str(text.0)
        .map_err(|error| GateError::parse(path, ErrorMessage::from(&error.to_string())))?;
    let elements = value
        .as_array()
        .ok_or_else(|| malformed("is an array of benchmark deltas"))?;
    let mut deltas = Vec::with_capacity(elements.len());
    for element in elements {
        let text = |field: &str| {
            element
                .get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    malformed("delta needs the strings `benchmark`, `before` and `after`")
                })
        };
        let regressed = match element
            .get("regressed")
            .and_then(serde_json::Value::as_bool)
        {
            Some(true) => Regressed::Yes,
            Some(false) => Regressed::No,
            None => return Err(malformed("delta needs a boolean `regressed`")),
        };
        deltas.push(Delta {
            benchmark: text("benchmark")?,
            before: text("before")?,
            after: text("after")?,
            regressed,
        });
    }
    Ok(Deltas(deltas))
}

/// Whether a pull request owes the hotpath verdict, and how it stands.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Verdict
{
    /// No report was given, or no benchmark regressed.
    #[default]
    NotOwed,
    /// A benchmark regressed.
    Owed
    {
        /// The verdict as written on the task line.
        written: String,
        /// How it stands.
        standing: Standing,
    },
}

/// How an owed hotpath verdict stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Standing
{
    /// The box is open.
    Unticked,
    /// The box is ticked and the verdict is in neither shape, or names no
    /// benchmark of the report.
    Malformed,
    /// The box is ticked and the verdict accepts a benchmark the report does
    /// not move.
    Unmoved,
    /// The box is ticked and the verdict is in one of the shapes.
    Settled,
}

impl Verdict
{
    /// Whether the verdict is owed and not settled.
    ///
    /// # Specification
    /// trivial.
    #[inline]
    #[must_use]
    pub const fn unsettled(&self) -> Unsettled
    {
        Unsettled(matches!(*self, Self::Owed {
            standing: Standing::Unticked | Standing::Malformed | Standing::Unmoved,
            ..
        }))
    }
}

/// Whether the report makes the hotpath verdict owed, and how it stands in
/// `body`.
///
/// # Specification
/// - ensures: [`Verdict::NotOwed`] when no report is given or no delta
///   regressed. Otherwise owed, from the last task line carrying
///   [`HOTPATH_MARKER`]: unticked while its box is open, settled when ticked
///   and [`verdict_shape`] reads it settled, else as that reads it; with no
///   such line, unticked with [`HOTPATH_TEMPLATE`].
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — no report, a report with nothing regressed, and a red
///   report over an empty body, an unticked line, each verdict shape ticked,
///   malformed verdicts and an `accepted` over an unmoved benchmark; owing
///   without a regression, settling an unticked box or accepting the unmoved
///   benchmark changes the standing.
/// - witness: `gates::obligations::a_red_hotpath_check_owes_a_verdict`
#[inline]
#[must_use]
#[spec(ensures: |ref output| matches!(*output, Verdict::NotOwed) != matches!(*report, Maybe::Present(ref deltas) if deltas.0.iter().any(|delta| delta.regressed == Regressed::Yes)))]
pub fn hotpath_verdict(
    report: &Maybe<Deltas, Unreported>,
    body: DocText<'_>,
) -> Verdict
{
    let Maybe::Present(ref deltas) = *report
    else {
        return Verdict::NotOwed;
    };
    if !deltas
        .0
        .iter()
        .any(|delta| delta.regressed == Regressed::Yes)
    {
        return Verdict::NotOwed;
    }
    let mut found = (HOTPATH_TEMPLATE.to_owned(), Standing::Unticked);
    for line in body.0.lines() {
        let line = line.trim();
        let ticked = line.starts_with("- [x]") || line.starts_with("- [X]");
        if !(ticked || line.starts_with("- [ ]")) {
            continue;
        }
        let Some((before, _)) = line.split_once(HOTPATH_MARKER)
        else {
            continue;
        };
        let written = before.get(5 ..).unwrap_or_default().trim().to_owned();
        let standing = if ticked {
            verdict_shape(DocText(&written), deltas)
        }
        else {
            Standing::Unticked
        };
        found = (written, standing);
    }
    Verdict::Owed {
        written: found.0,
        standing: found.1,
    }
}

/// Why no hotpath report was read: none was given.
///
/// # Specification
/// trivial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Unreported;

/// How a ticked `written` verdict stands against the report's `deltas`.
///
/// # Specification
/// - ensures: [`Standing::Settled`] for `hotpath: fixed — <benchmark> <before>
///   → <after>` naming a benchmark of the report with both numbers written, and
///   for `hotpath: accepted — <benchmark> exercises <feature>; <n> allocations
///   per <use>; <sentence>` with every part written, `<n>` in digits, naming a
///   benchmark whose before and after differ; [`Standing::Unmoved`] for that
///   `accepted` shape naming a benchmark the report does not move;
///   [`Standing::Malformed`] for anything else, placeholders and a benchmark
///   the report does not name included.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — each shape filled, each with a part missing, the
///   template, an `accepted` over the unmoved benchmark and one over a
///   benchmark the report lacks; any of them read as another standing changes
///   the obligation.
/// - witness: `gates::obligations::a_red_hotpath_check_owes_a_verdict`
#[inline]
#[must_use]
#[spec(ensures: |output| output != Standing::Settled || written.0.contains("hotpath: "))]
pub fn verdict_shape(
    written: DocText<'_>,
    deltas: &Deltas,
) -> Standing
{
    let written = written.0.trim().trim_matches('`').trim();
    let filled = |part: &str| InShape(!part.trim().is_empty() && !part.trim().starts_with('<'));
    let named = |benchmark: &str| deltas.0.iter().find(|delta| delta.benchmark == benchmark);
    if let Some(rest) = written.strip_prefix("hotpath: fixed — ") {
        let shaped = rest.split_once(' ').is_some_and(|(benchmark, numbers)| {
            named(benchmark).is_some()
                && numbers
                    .split_once(" → ")
                    .is_some_and(|(before, after)| filled(before).0 && filled(after).0)
        });
        return if shaped {
            Standing::Settled
        }
        else {
            Standing::Malformed
        };
    }
    let Some(rest) = written.strip_prefix("hotpath: accepted — ")
    else {
        return Standing::Malformed;
    };
    let parts: Vec<&str> = rest.splitn(3, "; ").collect();
    let [exercises, cost, sentence] = *parts.as_slice()
    else {
        return Standing::Malformed;
    };
    let shaped = exercises
        .split_once(" exercises ")
        .filter(|&(benchmark, feature)| filled(benchmark).0 && filled(feature).0)
        .zip(
            cost.split_once(" allocations per ")
                .filter(|&(count, used)| {
                    !count.is_empty()
                        && count.bytes().all(|byte| byte.is_ascii_digit())
                        && filled(used).0
                }),
        )
        .filter(|_| filled(sentence).0);
    let Some(((benchmark, _), _)) = shaped
    else {
        return Standing::Malformed;
    };
    match named(benchmark.trim()) {
        Some(delta) if delta.before != delta.after => Standing::Settled,
        Some(_) => Standing::Unmoved,
        None => Standing::Malformed,
    }
}

/// The hotpath verdict's task line and its instruction, for the body.
///
/// # Specification
/// trivial.
#[inline]
#[must_use]
pub fn verdict_lines(verdict: &Verdict) -> Vec<String>
{
    let Verdict::Owed {
        ref written,
        standing,
    } = *verdict
    else {
        return Vec::new();
    };
    let mut lines = vec![
        String::new(),
        "### Hotpath".to_owned(),
        String::new(),
        "This pull request's hotpath check reports a regression, so it states the regression \
         fixed or the growth accepted with its cost per use. Replace the text with `hotpath: \
         fixed — <benchmark> <before> → <after>` or `hotpath: accepted — <benchmark> exercises \
         <feature>; <n> allocations per <use>; <one sentence>`, then tick the box."
            .to_owned(),
        String::new(),
        format!(
            "- [{}] {written} {HOTPATH_MARKER}",
            if standing == Standing::Unticked {
                ' '
            }
            else {
                'x'
            }
        ),
    ];
    match standing {
        Standing::Malformed => lines.extend([
            String::new(),
            "⚠️ The ticked verdict is in neither shape, or names no benchmark of the report."
                .to_owned(),
        ]),
        Standing::Unmoved => lines.extend([
            String::new(),
            "⚠️ The verdict accepts a benchmark the report does not move: growth on an input the \
             change does not reach is fixed, not accepted."
                .to_owned(),
        ]),
        Standing::Unticked | Standing::Settled => {},
    }
    lines
}
