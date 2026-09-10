#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![cfg_attr(
    not(doc),
    doc = "Conditional specification attributes and token-preserving erasure."
)]

/// Select instrumentation in the annotated crate, not in the host dependency.
///
/// # Specification
/// - ensures: the consumer's `anodized` feature selects the published backend;
///   its absence selects nested-marker erasure.
/// - provides: original predicate and item tokens to the selected
///   interpretation.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — feature-separated integration cases distinguish
///   forwarding from stripping; negative predicates distinguish actual
///   enforcement.
/// - witness: `expansion::tests::disabled_preserves_nested_code_and_macro_languages`
/// - witness: `expansion::tests::nested_trait_obligations_follow_the_selected_mode`
#[inline]
#[proc_macro_attribute]
pub fn spec(
    arguments: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream
{
    let span = proc_macro::Span::call_site();
    let condition = proc_macro::TokenStream::from_iter([
        proc_macro::TokenTree::Ident(proc_macro::Ident::new("feature", span)),
        proc_macro::TokenTree::Punct(proc_macro::Punct::new('=', proc_macro::Spacing::Alone)),
        proc_macro::TokenTree::Literal(proc_macro::Literal::string("anodized")),
    ]);
    let mut output = conditional_attribute(
        condition.clone(),
        proc_macro::Ident::new("__instrument", span),
        arguments,
    );
    let disabled = proc_macro::TokenStream::from_iter([
        proc_macro::TokenTree::Ident(proc_macro::Ident::new("not", span)),
        proc_macro::TokenTree::Group(proc_macro::Group::new(
            proc_macro::Delimiter::Parenthesis,
            condition,
        )),
    ]);
    output.extend(conditional_attribute(
        disabled,
        proc_macro::Ident::new("__erase", span),
        proc_macro::TokenStream::new(),
    ));
    output.extend(item);
    output
}

/// Remove owned markers while retaining the ordinary token program.
///
/// # Specification
/// - requires: called by the facade's generated disabled branch.
/// - ensures: removes bare `spec` attributes outside uninterpreted macro
///   payloads.
/// - fails: nonempty arguments emit a compiler error.
/// - panics: none.
///
/// # Errors
/// Attribute arguments are rejected because this helper does not interpret
/// predicates.
///
/// # Adequacy
/// - hypothesis: L3 — nested declarations and macro-shaped data distinguish
///   erasure from accidental rewriting of the user's token language.
/// - witness: `expansion::tests::disabled_preserves_nested_code_and_macro_languages`
/// - witness: `expansion::tests::nested_trait_obligations_follow_the_selected_mode`
#[inline]
#[doc(hidden)]
#[proc_macro_attribute]
pub fn __erase(
    arguments: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream
{
    if !arguments.is_empty() {
        return proc_macro::TokenStream::from_iter([
            proc_macro::TokenTree::Ident(proc_macro::Ident::new(
                "compile_error",
                proc_macro::Span::call_site(),
            )),
            proc_macro::TokenTree::Punct(proc_macro::Punct::new('!', proc_macro::Spacing::Alone)),
            proc_macro::TokenTree::Group(proc_macro::Group::new(
                proc_macro::Delimiter::Brace,
                proc_macro::TokenStream::from(proc_macro::TokenTree::Literal(
                    proc_macro::Literal::string(
                        "the specification erasure helper does not accept arguments",
                    ),
                )),
            )),
        ]);
    }
    strip_markers(item)
}

/// Attach a qualified helper without reparsing the authored item or arguments.
///
/// # Specification
/// - ensures: the selected helper receives the original argument token stream.
/// - provides: one `cfg_attr` with the supplied consumer-side condition.
/// - panics: none.
fn conditional_attribute(
    mut condition: proc_macro::TokenStream,
    helper: proc_macro::Ident,
    arguments: proc_macro::TokenStream,
) -> proc_macro::TokenStream
{
    condition.extend([
        proc_macro::TokenTree::Punct(proc_macro::Punct::new(',', proc_macro::Spacing::Alone)),
        proc_macro::TokenTree::Punct(proc_macro::Punct::new(':', proc_macro::Spacing::Joint)),
        proc_macro::TokenTree::Punct(proc_macro::Punct::new(':', proc_macro::Spacing::Alone)),
        proc_macro::TokenTree::Ident(proc_macro::Ident::new(
            "quenchant",
            proc_macro::Span::call_site(),
        )),
        proc_macro::TokenTree::Punct(proc_macro::Punct::new(':', proc_macro::Spacing::Joint)),
        proc_macro::TokenTree::Punct(proc_macro::Punct::new(':', proc_macro::Spacing::Alone)),
        proc_macro::TokenTree::Ident(helper),
        proc_macro::TokenTree::Group(proc_macro::Group::new(
            proc_macro::Delimiter::Parenthesis,
            arguments,
        )),
    ]);
    let attribute = proc_macro::TokenStream::from_iter([
        proc_macro::TokenTree::Ident(proc_macro::Ident::new(
            "cfg_attr",
            proc_macro::Span::call_site(),
        )),
        proc_macro::TokenTree::Group(proc_macro::Group::new(
            proc_macro::Delimiter::Parenthesis,
            condition,
        )),
    ]);
    proc_macro::TokenStream::from_iter([
        proc_macro::TokenTree::Punct(proc_macro::Punct::new('#', proc_macro::Spacing::Alone)),
        proc_macro::TokenTree::Group(proc_macro::Group::new(
            proc_macro::Delimiter::Bracket,
            attribute,
        )),
    ])
}

/// A token cursor and the already visited output at one delimiter depth.
struct Frame
{
    /// Unvisited tokens; lookahead never advances past an uninterpreted
    /// payload.
    input: core::iter::Peekable<proc_macro::token_stream::IntoIter>,
    /// Visited ordinary tokens, excluding owned specification markers.
    output: proc_macro::TokenStream,
}

impl Frame
{
    /// Start a delimiter level before any of its tokens have been visited.
    ///
    /// # Specification
    /// trivial.
    fn new(input: proc_macro::TokenStream) -> Self
    {
        Self {
            input: input.into_iter().peekable(),
            output: proc_macro::TokenStream::new(),
        }
    }
}

/// Which token group closes the uninterpreted part of a macro construct.
#[derive(Clone, Copy)]
enum MacroBoundary
{
    /// An invocation or `macro_rules!` body accepts any delimiter.
    Group,
    /// A declarative `macro` item ends at its braced rule body.
    Body,
}

/// Retain macro-language data without applying Rust attribute interpretation.
///
/// # Specification
/// - ensures: copies through the selected payload group without descending into
///   it.
/// - provides: the unvisited suffix for ordinary traversal to resume.
/// - panics: none.
fn preserve_macro(
    frame: &mut Frame,
    boundary: MacroBoundary,
)
{
    for token in frame.input.by_ref() {
        let ends_payload = match token {
            | proc_macro::TokenTree::Group(ref group) => match boundary {
                | MacroBoundary::Group => true,
                | MacroBoundary::Body => group.delimiter() == proc_macro::Delimiter::Brace,
            },
            | _ => false,
        };
        frame.output.extend([token]);
        if ends_payload {
            break;
        }
    }
}

/// Whether a bracket group belongs to the enclosing specification annotation.
#[derive(Clone, Copy)]
enum Marker
{
    /// A bare `spec` path, with no arguments or one argument group.
    Specification,
    /// Another attribute, including qualified paths and conditional metadata.
    Other,
}

/// Recognize only the nested marker syntax accepted by the selected backend.
///
/// # Specification
/// - ensures: qualified names and unrelated metadata are never classified as
///   owned.
/// - panics: none.
fn marker(group: &proc_macro::Group) -> Marker
{
    let mut tokens = group.stream().into_iter();
    let Some(proc_macro::TokenTree::Ident(name)) = tokens.next()
    else {
        return Marker::Other;
    };
    if name.to_string() != "spec" {
        return Marker::Other;
    }
    match tokens.next() {
        | None => Marker::Specification,
        | Some(proc_macro::TokenTree::Group(arguments))
            if arguments.delimiter() == proc_macro::Delimiter::Parenthesis
                && tokens.next().is_none() =>
        {
            Marker::Specification
        },
        | _ => Marker::Other,
    }
}

/// Erase owned attributes with an explicit parent stack, not input-depth
/// recursion.
///
/// # Specification
/// - ensures: ordinary tokens and uninterpreted macro payloads retain their
///   order; only supported bare `spec` attributes are removed.
/// - provides: original token spans and original enclosing spans for rebuilt
///   groups.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — nested declarations, other attributes, and macro matcher
///   and invocation payloads distinguish the traversal's interpretation
///   boundaries.
/// - witness: `expansion::tests::disabled_preserves_nested_code_and_macro_languages`
/// - witness: `expansion::tests::nested_trait_obligations_follow_the_selected_mode`
fn strip_markers(item: proc_macro::TokenStream) -> proc_macro::TokenStream
{
    let mut frame = Frame::new(item);
    let mut parents: Vec<(proc_macro::Group, Frame)> = Vec::new();
    loop {
        let next = frame.input.next();
        let Some(token) = next
        else {
            let Some((original, parent)) = parents.pop()
            else {
                return frame.output;
            };
            let mut group = proc_macro::Group::new(original.delimiter(), frame.output);
            group.set_span(original.span());
            frame = parent;
            frame.output.extend([proc_macro::TokenTree::Group(group)]);
            continue;
        };
        match token {
            | proc_macro::TokenTree::Punct(punct) if punct.as_char() == '#' => {
                let attribute_kind = frame.input.peek().and_then(|token| match *token {
                    | proc_macro::TokenTree::Group(ref group)
                        if group.delimiter() == proc_macro::Delimiter::Bracket =>
                    {
                        Some(marker(group))
                    },
                    | _ => None,
                });
                if let Some(owned) = attribute_kind {
                    let attribute = frame.input.next();
                    if matches!(owned, Marker::Other) {
                        frame.output.extend([proc_macro::TokenTree::Punct(punct)]);
                        frame.output.extend(attribute);
                    }
                }
                else {
                    frame.output.extend([proc_macro::TokenTree::Punct(punct)]);
                }
            },
            | proc_macro::TokenTree::Ident(name) => {
                let macro_boundary = frame.input.peek().and_then(|token| match *token {
                    | proc_macro::TokenTree::Punct(ref punct) if punct.as_char() == '!' => {
                        Some(MacroBoundary::Group)
                    },
                    | proc_macro::TokenTree::Ident(_) if name.to_string() == "macro" => {
                        Some(MacroBoundary::Body)
                    },
                    | _ => None,
                });
                frame.output.extend([proc_macro::TokenTree::Ident(name)]);
                if let Some(boundary) = macro_boundary {
                    preserve_macro(&mut frame, boundary);
                }
            },
            | proc_macro::TokenTree::Group(group) => {
                let nested = Frame::new(group.stream());
                parents.push((group, frame));
                frame = nested;
            },
            | token => frame.output.extend([token]),
        }
    }
}
