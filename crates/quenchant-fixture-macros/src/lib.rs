//! Attribute macros the Dylint UI matrix expands.
//!
//! A gate that exempts foreign expansions has to be read against a real
//! foreign expansion, and the shapes that matter are the ones a third-party
//! macro produces rather than the ones a fixture can write by hand. This crate
//! is that third party: it lives outside the crate under test, so rustc treats
//! its expansions as external, and it is a workspace member so the lint wall
//! covers its own source.
//!
//! One shape ships here. [`client`] re-emits the annotated inherent `impl`
//! unchanged and generates a client `impl` beside it whose methods are named by
//! passing the author's own method identifiers through. The generated methods
//! carry no rustdoc and never could: their names read as the author's syntax
//! while their declarations are the macro's.

use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;

/// A Rust fragment this crate's own source spells out.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct MacroSource<'source>(&'source str);

/// Generate a client `impl` whose methods reuse the author's identifiers.
///
/// # Specification
/// - requires: the annotated item is an inherent `impl` whose body declares
///   methods.
/// - ensures: re-emits the annotated item unchanged and appends a unit struct
///   named after the subject and an `impl` on it carrying one method per
///   declared method, each named by the author's own identifier token; an item
///   that is not an `impl` with a body is re-emitted alone.
/// - provides: the foreign expansion the presence gate's UI matrix reads a
///   manufactured declaration off.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the expansion is read for the two properties the matrix
///   depends on, that the generated method's name is the author's own token and
///   that the client subject is derived from the author's, plus the arm that
///   declines an item it cannot read.
/// - witness: `tests::the_generated_method_takes_the_author_identifier`
/// - witness: `tests::an_unreadable_item_is_re_emitted_alone`
#[inline]
#[proc_macro_attribute]
pub fn client(
    _attribute: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream
{
    proc_macro::TokenStream::from(client_expansion(TokenStream::from(item)))
}

/// Return the annotated item followed by its generated client `impl`.
///
/// # Specification
/// - requires: `item` is the token stream of the annotated item.
/// - ensures: returns `item` alone when no `impl` subject or brace-delimited
///   body is found, and otherwise returns `item` followed by a unit struct and
///   an `impl` on it holding one `pub fn <name>(&self) {}` per method the body
///   declares, where `<name>` is the author's own identifier token and every
///   other token is this macro's own.
/// - provides: the expansion [`client`] emits, over the token type a test can
///   build outside a compiler.
/// - panics: none.
///
/// # Adequacy
/// - hypothesis: L3 — the tests separate the generated method's name token from
///   the macro's own tokens, the derived subject name, and the declined item.
/// - witness: `tests::the_generated_method_takes_the_author_identifier`
/// - witness: `tests::an_unreadable_item_is_re_emitted_alone`
fn client_expansion(item: TokenStream) -> TokenStream
{
    let mut subject = None;
    let mut body = None;
    let mut after_impl = false;
    for tree in item.clone() {
        match tree {
            | TokenTree::Ident(ident) if ident == "impl" => after_impl = true,
            | TokenTree::Ident(ident) if after_impl => {
                subject = Some(ident);
                after_impl = false;
            },
            | TokenTree::Group(group) if group.delimiter() == Delimiter::Brace => {
                body = Some(group);
            },
            | _ => {},
        }
    }
    let (Some(subject), Some(body)) = (subject, body)
    else {
        return item;
    };
    let mut methods = TokenStream::new();
    let mut after_fn = false;
    for tree in body.stream() {
        match tree {
            | TokenTree::Ident(ident) if ident == "fn" => after_fn = true,
            | TokenTree::Ident(ident) if after_fn => {
                after_fn = false;
                methods.extend(parsed(MacroSource("pub fn")));
                methods.extend(TokenStream::from(TokenTree::Ident(ident)));
                methods.extend(parsed(MacroSource("(&self) {}")));
            },
            | _ => {},
        }
    }
    let mut expansion = item;
    expansion.extend(parsed(MacroSource(&format!("struct {subject}Client;"))));
    expansion.extend(parsed(MacroSource(&format!("impl {subject}Client"))));
    expansion.extend(TokenStream::from(TokenTree::Group(Group::new(
        Delimiter::Brace,
        methods,
    ))));
    expansion
}

/// Return the tokens of a source fragment this macro writes itself.
///
/// # Specification
/// - requires: `source` is a Rust fragment this crate's own source spells out.
/// - ensures: returns its tokens, each carrying this macro's call site rather
///   than any of the author's spans, and an empty stream for a fragment that
///   does not lex — which no caller here writes, so an empty expansion becomes
///   the compiler's own syntax error rather than a silent success.
/// - provides: the macro-owned half of every generated item.
/// - panics: none.
fn parsed(source: MacroSource<'_>) -> TokenStream
{
    source.0.parse::<TokenStream>().unwrap_or_default()
}

#[cfg(test)]
mod tests
{
    use super::MacroSource;
    use super::client_expansion;
    use super::parsed;

    #[test]
    fn the_generated_method_takes_the_author_identifier()
    {
        let expansion =
            client_expansion(parsed(MacroSource("impl Turn { pub fn change(&self) {} }")))
                .to_string();
        assert!(
            expansion.contains("struct TurnClient"),
            "the client subject is derived from the author's: {expansion}"
        );
        assert!(
            expansion.contains("impl TurnClient"),
            "the generated impl is on the client subject: {expansion}"
        );
        assert_eq!(
            expansion.matches("change").count(),
            2_usize,
            "the author's method and its generated sibling share one name: {expansion}"
        );
    }

    #[test]
    fn an_unreadable_item_is_re_emitted_alone()
    {
        let expansion = client_expansion(parsed(MacroSource("fn free() {}"))).to_string();
        assert!(
            !expansion.contains("Client"),
            "an item with no impl subject gains nothing: {expansion}"
        );
        assert!(
            expansion.contains("fn free"),
            "the declined item is still re-emitted: {expansion}"
        );
    }
}
