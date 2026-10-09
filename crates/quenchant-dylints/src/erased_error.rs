//! A crate-defined signature names the error it returns.
//!
//! `Result<T, Box<dyn Error>>`, `anyhow::Error` and `eyre::Report` tell the
//! caller that something failed and not which failure: the variants a caller
//! would match on are erased at the boundary. The discipline requires a typed
//! error at every crate-defined boundary, so a returned `Result` whose error is
//! one of these is refused.
//!
//! Test code is where an erased error belongs: a test reports a failure and
//! never matches on it. A `--test` compilation is exempt as a whole; the
//! library half of a unit-test build is checked again by its ordinary build,
//! and an integration-test crate has no other build. A method implementing a
//! foreign trait answers to that trait's signature and is exempt too.

use core::cell::OnceCell;

use clippy_utils::diagnostics::span_lint_and_help;
use rustc_hir::Body;
use rustc_hir::FnDecl;
use rustc_hir::TraitFn;
use rustc_hir::TraitItem;
use rustc_hir::TraitItemKind;
use rustc_hir::def_id::DefId;
use rustc_hir::def_id::LocalDefId;
use rustc_hir::intravisit::FnKind;
use rustc_lint::LateContext;
use rustc_lint::LateLintPass;
use rustc_middle::ty;
use rustc_middle::ty::Ty;
use rustc_session::declare_lint;
use rustc_session::impl_lint_pass;
use rustc_span::Span;
use rustc_span::Symbol;
use rustc_span::sym;

use crate::semantic::ErasedError;
use crate::specification::name_authored;

declare_lint! {
    /// Reject a crate-defined signature that returns an erased error.
    ///
    /// ### What it does
    ///
    /// Reports, at the return type, a crate-authored function or method whose
    /// output reaches a `Result` whose error type is `dyn Error` behind `Box`,
    /// `Arc`, `Rc` or a reference, `anyhow::Error`, or `eyre::Report`. The
    /// output is followed through aliases, nested types and the outputs an
    /// `impl Trait` or `async fn` declares.
    ///
    /// ### Why is this bad?
    ///
    /// The caller can no longer tell which failure occurred, so every
    /// consumer re-derives it from a message or gives up. A named error enum
    /// keeps the failures matchable at the boundary.
    ///
    /// ### What it does not decide
    ///
    /// Parameters are not inspected: a function that accepts any error, to
    /// report it, erases nothing it returns. A named error that carries an
    /// erased source is a named error. A `--test` compilation and a method
    /// implementing a foreign trait are exempt; a foreign trait's associated
    /// error type is chosen at the implementation and is not read here.
    ///
    /// ### Example
    ///
    /// ```rust,ignore
    /// fn load(path: &Path) -> Result<Config, Box<dyn Error>>;
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust,ignore
    /// fn load(path: &Path) -> Result<Config, LoadError>;
    /// ```
    pub ERASED_ERROR_SIGNATURE,
    Allow,
    "a crate-defined signature returns a named error type rather than an erased one"
}

/// Erased error types named by path. Each is a dependency's type, resolved
/// only when that dependency is linked.
const ERASED_ERROR_TYPES: [&str; 2] = ["anyhow::Error", "eyre::Report"];

/// Signature-level error policy; the erased types are resolved once per crate.
#[derive(Default)]
#[repr(transparent)]
pub struct ErasedErrorSignature
{
    /// The definitions of every erased error type among this crate's
    /// dependencies, resolved on the first inspected signature.
    erased: OnceCell<Vec<DefId>>,
}

impl_lint_pass!(ErasedErrorSignature => [ERASED_ERROR_SIGNATURE]);

impl<'tcx> LateLintPass<'tcx> for ErasedErrorSignature
{
    /// Inspect body-bearing declarations without treating closures as APIs.
    ///
    /// # Specification
    /// - ensures: checks free functions, inherent methods, and local trait
    ///   methods with bodies; closure signatures are inferred implementation
    ///   details.
    /// - panics: none.
    fn check_fn(
        &mut self,
        cx: &LateContext<'tcx>,
        kind: FnKind<'tcx>,
        declaration: &'tcx FnDecl<'tcx>,
        _: &'tcx Body<'tcx>,
        _: Span,
        owner: LocalDefId,
    )
    {
        if !matches!(kind, FnKind::Closure) {
            self.check_declaration(cx, declaration, owner);
        }
    }

    /// Required local trait methods own signatures even without bodies.
    ///
    /// # Specification
    /// trivial.
    fn check_trait_item(
        &mut self,
        cx: &LateContext<'tcx>,
        item: &'tcx TraitItem<'tcx>,
    )
    {
        if let TraitItemKind::Fn(signature, TraitFn::Required(_)) = item.kind {
            self.check_declaration(cx, signature.decl, item.owner_id.def_id);
        }
    }
}

impl ErasedErrorSignature
{
    /// One signature is reported at most once, at its return type.
    ///
    /// # Specification
    /// - ensures: reports once, at the written return type, a crate-authored
    ///   signature outside a `--test` compilation and outside a foreign trait's
    ///   implementation whose output reaches a `Result` with an erased error;
    ///   does nothing where the lint is allowed.
    /// - panics: none.
    ///
    /// # Adequacy
    /// - hypothesis: L3 — the error matrix separates boxed, shared, aliased,
    ///   nested, asynchronous, `impl Trait`, required-trait and dependency
    ///   erasures (reported) from a named error, a named error carrying an
    ///   erased source, an erased parameter, a foreign trait's implementation
    ///   and a `--test` compilation (accepted).
    /// - witness: `tests::ui_shapes`
    fn check_declaration<'tcx>(
        &self,
        cx: &LateContext<'tcx>,
        declaration: &'tcx FnDecl<'tcx>,
        owner: LocalDefId,
    )
    {
        if cx.tcx.sess.opts.test
            || cx
                .tcx
                .lint_level_spec_at_node(
                    ERASED_ERROR_SIGNATURE,
                    cx.tcx.local_def_id_to_hir_id(owner),
                )
                .level()
                == rustc_session::lint::Level::Allow
            || !name_authored(cx, owner).0
            || crate::implements_non_local_trait(cx, owner).0
        {
            return;
        }
        let erased = self.erased.get_or_init(|| erased_error_types(cx));
        let output = cx
            .tcx
            .fn_sig(owner)
            .instantiate_identity()
            .skip_norm_wip()
            .skip_binder()
            .output();
        if returns_erased_error(cx, erased, output).0 {
            span_lint_and_help(
                cx,
                ERASED_ERROR_SIGNATURE,
                declaration.output.span(),
                "this signature returns a `Result` whose error is erased, so a caller cannot tell \
                 which failure occurred",
                None,
                "return a named error enum whose variants are the failures this function can \
                 produce; an erased error belongs in test code",
            );
        }
    }
}

/// Erased dependency types are resolved once per crate.
///
/// # Specification
/// - ensures: returns the definition of `anyhow::Error` and of `eyre::Report`
///   among this crate's dependencies, each once; a crate that links neither
///   yields none.
/// - panics: none.
fn erased_error_types(cx: &LateContext<'_>) -> Vec<DefId>
{
    let mut types = Vec::new();
    for path in ERASED_ERROR_TYPES {
        for def_id in
            clippy_utils::paths::lookup_path_str(cx.tcx, clippy_utils::paths::PathNS::Type, path)
        {
            if !types.contains(&def_id) {
                types.push(def_id);
            }
        }
    }
    types
}

/// A `Result` anywhere in the output, or in what an opaque output declares,
/// is read for an erased error.
///
/// # Specification
/// - requires: `erased` is [`erased_error_types`] for this crate.
/// - ensures: affirmative exactly when `output`, its type arguments, or the
///   projection outputs an opaque type among them declares, reach a `Result`
///   whose error type [`erases`]; each opaque type is opened once, so the walk
///   ends on any signature.
/// - panics: none.
fn returns_erased_error<'tcx>(
    cx: &LateContext<'tcx>,
    erased: &[DefId],
    output: Ty<'tcx>,
) -> ErasedError
{
    let mut pending = vec![output];
    let mut opened: Vec<DefId> = Vec::new();
    while let Some(next) = pending.pop() {
        for argument in next.walk() {
            let Some(inner) = argument.as_type()
            else {
                continue;
            };
            match *inner.kind() {
                | ty::Adt(adt, arguments) if cx.tcx.is_diagnostic_item(sym::Result, adt.did()) => {
                    if arguments
                        .types()
                        .nth(1)
                        .is_some_and(|error| erases(cx, erased, error).0)
                    {
                        return ErasedError(true);
                    }
                },
                | ty::Alias(ty::AliasTy {
                    kind: ty::Opaque { def_id },
                    args,
                    ..
                }) if !opened.contains(&def_id) => {
                    opened.push(def_id);
                    for (clause, _) in cx
                        .tcx
                        .explicit_item_bounds(def_id)
                        .iter_instantiated_copied(cx.tcx, args)
                        .map(ty::Unnormalized::skip_norm_wip)
                    {
                        if let Some(projection) = clause.as_projection_clause()
                            && let Some(term) = projection.skip_binder().term.as_type()
                        {
                            pending.push(term);
                        }
                    }
                },
                | _ => {},
            }
        }
    }
    ErasedError(false)
}

/// An error type is erased when it is a trait object of `Error` behind a
/// pointer, or a dependency's erased error type.
///
/// # Specification
/// - requires: `erased` is [`erased_error_types`] for this crate.
/// - ensures: affirmative exactly when `error`, after peeling `Box`, `Arc`,
///   `Rc` and references, is a trait object whose principal trait is
///   `core::error::Error`, or is an ADT among `erased`; a named error that
///   carries an erased value in a field is not erased.
/// - panics: none.
fn erases<'tcx>(
    cx: &LateContext<'tcx>,
    erased: &[DefId],
    error: Ty<'tcx>,
) -> ErasedError
{
    let mut current = error;
    loop {
        match *current.kind() {
            | ty::Ref(_, inner, _) => current = inner,
            | ty::Adt(adt, arguments)
                if adt.is_box()
                    || cx.tcx.is_diagnostic_item(sym::Arc, adt.did())
                    || cx.tcx.is_diagnostic_item(sym::Rc, adt.did()) =>
            {
                let Some(inner) = arguments.types().next()
                else {
                    return ErasedError(false);
                };
                current = inner;
            },
            | ty::Adt(adt, _) => return ErasedError(erased.contains(&adt.did())),
            | ty::Dynamic(predicates, ..) => {
                return ErasedError(predicates.principal_def_id().is_some_and(|principal| {
                    cx.tcx
                        .is_diagnostic_item(Symbol::intern("Error"), principal)
                }));
            },
            | _ => return ErasedError(false),
        }
    }
}
