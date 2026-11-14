//! A trait object stands only where its site states why.
//!
//! A `dyn` type dispatches each call through a table and boxes each value it
//! owns. Where the compiler knows the set of implementers, a closed enum or a
//! generic parameter does the same work with no table and no box. So the rule
//! refuses each `dyn` type whose principal trait is in a configured class. A
//! consumer turns the rule on class by class: each class is the path of a
//! trait, and the class `*` takes every trait object, auto traits alone too.
//!
//! # Boundaries
//!
//! A site that keeps its trait object states why with
//! `#[expect(trait_object, reason = "…")]` at the site or on a node between it
//! and its enclosing item. An expectation on an enclosing module,
//! implementation or crate, one without its reason and an `allow` written in
//! the source are refused. The rule reads a test as it reads a library: a
//! type under `#[cfg(test)]` and a type in a test target are refused unless
//! their site states an expectation. So one expectation holds in the library
//! build and in its test build. A type that an external macro expands is not
//! read.

use anodized::spec;
use quenchant_shape::shape::Maybe;
use rustc_hir::AmbigArg;
use rustc_hir::Ty;
use rustc_hir::TyKind;
use rustc_hir::def_id::DefId;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_lint::declare_lint;
use rustc_lint::impl_lint_pass;
use rustc_middle::ty::TyCtxt;

use crate::answer_totality::CONFIGURATION_TABLE;
use crate::answer_totality::ExceptionRefusals;
use crate::answer_totality::names_local_crate;
use crate::answer_totality::report_defects;
use crate::answer_totality::report_refusing;
use crate::answer_totality::resolve;
use crate::semantic::DiagnosticText;
use crate::semantic::TypePath;

declare_lint! {
    /// ### What it does
    ///
    /// Refuses a `dyn` type whose principal trait is in a configured class.
    ///
    /// ### Why is this bad?
    ///
    /// A trait object dispatches each call through a table and boxes each
    /// value it owns. Where the compiler knows the implementers, a closed enum
    /// or a generic parameter does the same work with neither.
    ///
    /// ### Configuration
    ///
    /// `trait_object_classes` under `[quenchant-dylints]`: the paths of the
    /// traits whose objects the rule refuses, or `*` for every trait object.
    /// Without the key the rule reads nothing.
    ///
    /// ### The exception
    ///
    /// A site that keeps its trait object states why:
    /// `#[expect(trait_object, reason = "...")]` at the site or on a node
    /// between it and its enclosing item.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// fn parse(text: &str) -> Result<Tree, Box<dyn Error>>;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn parse(text: &str) -> Result<Tree, ParseError>;
    /// ```
    pub TRAIT_OBJECT,
    Deny,
    "a trait object of a configured class stands only under a reasoned expectation at its site"
}

impl_lint_pass!(TraitObject => [TRAIT_OBJECT]);

quenchant_shape::reason_enum! {
    /// A type can be no trait object of a configured class.
    mod object_reading {
        /// Why the type is not refused.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Unrefused {
            /// The type is no trait object.
            NotAnObject,
            /// The trait object's principal trait is in no configured class.
            OtherClass,
        }
    }
}

/// The class that takes every trait object.
const EVERY_CLASS: &str = "*";

/// The configuration key of the classes.
const CLASSES_KEY: &str = "trait_object_classes";

/// The help every finding carries.
const HELP: &str = concat!(
    "name the implementers: a closed enum where the set is known, a generic parameter where ",
    "the callers are few; a site that keeps its trait object states why with ",
    "`#[expect(trait_object, reason = \"...\")]`",
);

/// The refusals of an exception the site does not state properly.
const REFUSALS: ExceptionRefusals = ExceptionRefusals {
    inherited: DiagnosticText(concat!(
        "a trait-object exception must be stated where the trait object stands: an ",
        "`#[expect(trait_object, ..)]` on an enclosing module, implementation or crate excepts ",
        "nothing inside it",
    )),
    unreasoned: DiagnosticText(concat!(
        "a trait-object exception must state why the site keeps its trait object: write ",
        "`reason = \"...\"` on its `#[expect(trait_object, ..)]`",
    )),
    allowed: DiagnosticText(concat!(
        "a trait object of a configured class is admitted only by ",
        "`#[expect(trait_object, reason = \"...\")]`; an `allow` does not say why the site keeps ",
        "it, nor fail when it goes",
    )),
};

/// The configured classes, as one crate reads them.
#[derive(Default)]
pub struct TraitObject
{
    /// Whether the class `*` takes every trait object.
    every: Every,
    /// The principal traits of the configured classes that this crate sees.
    classes: Vec<DefId>,
    /// Entries that the configuration states wrongly or that resolve to
    /// nothing in the crate they name.
    defects: Vec<String>,
}

/// Whether the class `*` is configured.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Every
{
    /// The class `*` is not configured.
    #[default]
    Absent,
    /// The class `*` takes every trait object.
    Present,
}

/// The name a finding gives a trait object: its principal trait's path, or
/// the auto traits alone.
#[repr(transparent)]
struct ObjectName(String);

impl TraitObject
{
    /// Read the configured classes as the crate `tcx` compiles reads them.
    ///
    /// # Specification
    /// - requires: `dylint_linting::init_config` has already run, which
    ///   [`crate::register_lints`] guarantees by calling it first.
    /// - ensures: the entry `*` takes every trait object; each other entry's
    ///   definitions that are traits are read as classes; a non-string entry,
    ///   an empty entry, and an entry that names this crate and resolves to no
    ///   trait become defects.
    /// - fails: an absent, unreadable or malformed configuration yields no
    ///   class, which reads nothing.
    /// - panics: none.
    /// - executable: none — the configuration read outcome is not retained;
    ///   reading it again is a distinct fallible operation.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures configure `Error` and `Fn` by path and
    ///   refuse their objects while a `Display` object passes; the class `*`
    ///   refuses an object of auto traits alone; the refused configuration
    ///   carries a number, an empty entry and a misspelt trait of the fixture
    ///   crate, each reported.
    /// - witness: `tests::ui_trait_object`
    /// - witness: `tests::ui_trait_object_every`
    /// - witness: `tests::ui_trait_object_refused`
    pub fn configured(tcx: TyCtxt<'_>) -> Self
    {
        let mut table = Self::default();
        let Ok(Some(config)) = dylint_linting::config_toml(CONFIGURATION_TABLE)
        else {
            return table;
        };
        for entry in config
            .get(CLASSES_KEY)
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
        {
            match entry.as_str() {
                Some(EVERY_CLASS) => table.every = Every::Present,
                Some(path) if !path.is_empty() => {
                    let traits: Vec<DefId> = resolve(tcx, TypePath(path))
                        .into_iter()
                        .filter(|definition| tcx.is_trait(*definition))
                        .collect();
                    if traits.is_empty() && names_local_crate(tcx, TypePath(path)).0 {
                        table.defects.push(format!(
                            "`{CLASSES_KEY}` entry `{path}` names no trait of this crate"
                        ));
                    }
                    table.classes.extend(traits);
                },
                Some(_) | None => table.defects.push(format!(
                    "`{CLASSES_KEY}` entry `{entry}` is not the path of a trait, nor `{EVERY_CLASS}`"
                )),
            }
        }
        table
    }

    /// The name of the trait object `ty` when a configured class takes it.
    ///
    /// # Specification
    /// - ensures: `Present` when `ty` is a trait object and either the class
    ///   `*` is configured or its principal trait, its first bound that is no
    ///   auto trait, is a configured class; the name is the principal trait's
    ///   path, or the auto traits' paths joined by `+` when it has none.
    ///   `Absent` otherwise.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures write objects of configured traits
    ///   behind a box, a reference and a generic argument, with an auto trait
    ///   added, and objects of an unconfigured trait and of auto traits alone;
    ///   a wrong principal or class changes a verdict.
    /// - witness: `tests::ui_trait_object`
    /// - witness: `tests::ui_trait_object_every`
    #[spec(ensures: |output| match output {
        Maybe::Present(_) => matches!(ty.kind, TyKind::TraitObject(..)),
        Maybe::Absent(_) => true,
    })]
    fn refused(
        &self,
        cx: &LateContext<'_>,
        ty: &Ty<'_>,
    ) -> Maybe<ObjectName, object_reading::Unrefused>
    {
        let TyKind::TraitObject(bounds, _) = ty.kind
        else {
            return Maybe::Absent(object_reading::Unrefused::NotAnObject);
        };
        let principal = bounds.iter().find_map(|bound| {
            bound
                .trait_ref
                .trait_def_id()
                .filter(|definition| !cx.tcx.trait_is_auto(*definition))
        });
        let taken = self.every == Every::Present
            || principal.is_some_and(|definition| self.classes.contains(&definition));
        if !taken {
            return Maybe::Absent(object_reading::Unrefused::OtherClass);
        }
        let name = principal.map_or_else(
            || {
                bounds
                    .iter()
                    .filter_map(|bound| bound.trait_ref.trait_def_id())
                    .map(|definition| cx.tcx.def_path_str(definition))
                    .collect::<Vec<_>>()
                    .join(" + ")
            },
            |definition| cx.tcx.def_path_str(definition),
        );
        Maybe::Present(ObjectName(name))
    }
}

impl<'tcx> LateLintPass<'tcx> for TraitObject
{
    /// Report the configuration's defects once per crate.
    ///
    /// # Specification
    /// - ensures: reports each defect [`TraitObject::configured`] recorded at
    ///   the crate root.
    /// - panics: none.
    /// - executable: none — rustc owns diagnostic emission; no readback exists.
    ///
    /// # Adequacy
    /// - hypothesis: L2 — the refused configuration's defects are reported in
    ///   the fixture crate.
    /// - witness: `tests::ui_trait_object_refused`
    fn check_crate(
        &mut self,
        cx: &LateContext<'tcx>,
    )
    {
        report_defects(cx, TRAIT_OBJECT, &self.defects);
    }

    /// Refuse a trait object of a configured class, in a test as in a library.
    ///
    /// # Specification
    /// - ensures: reports `ty` exactly when [`TraitObject::refused`] names it
    ///   and `ty` sits in no external macro's expansion, whether `ty` is under
    ///   `#[cfg(test)]`, in a test target or in neither; an exception the site
    ///   does not state properly is refused at the crate root
    ///   ([`report_refusing`]).
    /// - panics: none.
    /// - executable: none — rustc owns resolution and diagnostic emission.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the fixtures refuse each object of a configured
    ///   class, accept a reasoned expectation, and refuse an unreasoned
    ///   expectation, an inherited one and an `allow`; in a test target, a
    ///   reasoned expectation is fulfilled, and an unstated object is refused
    ///   in a library item, in a test function and under `#[cfg(test)]`.
    /// - witness: `tests::ui_trait_object`
    /// - witness: `tests::ui_trait_object_test_target`
    fn check_ty(
        &mut self,
        cx: &LateContext<'tcx>,
        ty: &'tcx Ty<'tcx, AmbigArg>,
    )
    {
        let ty = ty.as_unambig_ty();
        let Maybe::Present(name) = self.refused(cx, ty)
        else {
            return;
        };
        if ty.span.in_external_macro(cx.tcx.sess.source_map()) {
            return;
        }
        report_refusing(
            cx,
            TRAIT_OBJECT,
            ty.hir_id,
            ty.span,
            format!("`dyn {}` is a trait object of a configured class", name.0),
            DiagnosticText(HELP),
            &REFUSALS,
        );
    }
}
