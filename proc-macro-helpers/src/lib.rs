#![doc(html_playground_url = "https://play.rust-lang.org/")]
//! Frunk Proc Macro internals
//!
//! This library holds common logic for procedural macros used by frunk
//!
//! Links:
//!   1. [Source on Github](https://github.com/lloydmeta/frunk)
//!   2. [Crates.io page](https://crates.io/crates/frunk)

extern crate frunk_core;
extern crate proc_macro;
extern crate proc_macro2;

#[macro_use]
extern crate quote;
extern crate syn;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::ToTokens;
use quote::__rt::Span;
use syn::spanned::Spanned;
use syn::{
    DeriveInput, Expr, Field, Fields, GenericParam, Generics, Ident, Lifetime, LifetimeDef, Member,
};

/// These are assumed to exist as enums in frunk_core::labelled
const ALPHA_CHARS: &'static [char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's',
    't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L',
    'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

/// These are assumed to exist as enums in frunk_core::labelled as underscore prepended enums
const UNDERSCORE_CHARS: &'static [char] = &['_', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];

/// Parses a TokenStream (usually received as input into a
/// custom derive function), into a syn MacroInput AST,
/// which is nice.
pub fn to_ast(input: TokenStream) -> DeriveInput {
    // Parse the string representation
    syn::parse(input).unwrap()
}

/// Returns an Ident
pub fn call_site_ident(s: &str) -> Ident {
    Ident::new(s, Span::call_site())
}

/// Given a list of types, creates an AST for the corresponding HList
/// type.
pub fn build_hlist_type<L: IntoIterator>(items: L) -> TokenStream2
where
    L::Item: ToTokens,
    L::IntoIter: DoubleEndedIterator,
{
    let mut result = quote! { ::frunk_core::hlist::HNil };
    for item in items.into_iter().rev() {
        result = quote! { ::frunk_core::hlist::HCons<#item, #result> }
    }
    result
}

/// Given a list of expressions or patterns, creates an AST for the corresponding HList
/// constructor, which may itself be used as an expression or a pattern.
pub fn build_hlist_constr<L: IntoIterator>(items: L) -> TokenStream2
where
    L::Item: ToTokens,
    L::IntoIter: DoubleEndedIterator,
{
    let mut result = quote! { ::frunk_core::hlist::HNil };
    for item in items.into_iter().rev() {
        result = quote! { ::frunk_core::hlist::HCons { head: #item, tail: #result }}
    }
    result
}

/// Given an Ident returns an AST for its type level representation based on the
/// enums generated in frunk_core::labelled.
///
/// For example, given first_name, returns an AST for (f,i,r,s,t,__,n,a,m,e)
pub fn build_label_type(ident: &Ident) -> impl ToTokens {
    let as_string = ident.to_string();
    let name = as_string.as_str();
    let name_as_idents: Vec<Ident> = name.chars().flat_map(|c| encode_as_ident(&c)).collect();
    let name_as_tokens: Vec<_> = name_as_idents
        .iter()
        .map(|ident| {
            quote! { ::frunk_core::labelled::chars::#ident }
        })
        .collect();
    quote! { (#(#name_as_tokens),*) }
}

/// Given a char, encodes it as a vector of Ident
///
/// Takes care of checking to see whether the char can be used as is,
/// or needs to be encoded as an underscored character (_ and numbers),
/// or needs to be encoded as a unicode.
///
/// This method assumes that _uc and uc_ are in frunk_core::labelled as enums
fn encode_as_ident(c: &char) -> Vec<Ident> {
    if ALPHA_CHARS.contains(c) {
        vec![call_site_ident(&c.to_string())]
    } else if UNDERSCORE_CHARS.contains(c) {
        vec![call_site_ident(&format!("_{}", c))]
    } else {
        // UTF escape and get the hexcode
        let as_unicode = c.escape_unicode();
        // as_unicode can be multiple unicode codepoints encoded as \u{2764}\u{fe0f} (❤️)
        // so we filter on alphanumeric to get just u's as a delimiters along with the
        // hex portions
        let delimited_hex = as_unicode.filter(|c| c.is_alphanumeric());
        let mut hex_idents: Vec<Ident> = delimited_hex.flat_map(|c| encode_as_ident(&c)).collect();
        // sandwich between _uc and uc_
        let mut book_ended: Vec<Ident> = vec![call_site_ident("_uc")];
        book_ended.append(&mut hex_idents);
        book_ended.push(call_site_ident("uc_"));
        book_ended
    }
}

pub fn build_path_type(path_expr: Expr) -> impl ToTokens {
    let idents = find_idents_in_expr(path_expr);
    idents
        .iter()
        .map(|i| build_label_type(i))
        .fold(quote!(::frunk_core::hlist::HNil), |acc, t| {
            quote! {
            ::frunk_core::path::Path<
                ::frunk_core::hlist::HCons<
                   #t,
                   #acc
                >
              >
            }
        })
}

/// Returns the idents in a path like expression in reverse
pub fn find_idents_in_expr(path_expr: Expr) -> Vec<Ident> {
    fn go(current: Expr, mut v: Vec<Ident>) -> Vec<Ident> {
        match current {
            Expr::Field(e) => {
                let m = e.member;
                match m {
                    Member::Named(i) => {
                        v.push(i);
                    }
                    _ => panic!("Only named access is supported"),
                }
                go(*e.base, v)
            }
            Expr::Path(p) => {
                if p.path.segments.len() != 1 {
                    panic!("Invalid name; this has collons in it")
                } else {
                    let i = p.path.segments[0].ident.clone();
                    v.push(i);
                    v
                }
            }
            _ => panic!("Invalid input"),
        }
    }
    go(path_expr, Vec::new())
}

pub enum StructType {
    Named,
    Tuple,
    Unit,
}

pub struct FieldBinding {
    pub field: Field,
    pub binding: Ident,
}

impl FieldBinding {
    pub fn build_type(&self) -> TokenStream2 {
        let ty = &self.field.ty;
        quote! { #ty }
    }
    pub fn build_type_ref(&self) -> TokenStream2 {
        let ty = &self.field.ty;
        quote! { &'_frunk_ref_ #ty }
    }
    pub fn build_type_mut(&self) -> TokenStream2 {
        let ty = &self.field.ty;
        quote! { &'_frunk_ref_ mut #ty }
    }
    pub fn build_field_type(&self) -> TokenStream2 {
        let label_type = build_label_type(&self.binding);
        let ty = &self.field.ty;
        quote! { ::frunk_core::labelled::Field<#label_type, #ty> }
    }
    pub fn build_field_type_ref(&self) -> TokenStream2 {
        let label_type = build_label_type(&self.binding);
        let ty = &self.field.ty;
        quote! { ::frunk_core::labelled::Field<#label_type, &'_frunk_ref_ #ty> }
    }
    pub fn build_field_type_mut(&self) -> TokenStream2 {
        let label_type = build_label_type(&self.binding);
        let ty = &self.field.ty;
        quote! { ::frunk_core::labelled::Field<#label_type, &'_frunk_ref_ mut #ty> }
    }
    pub fn build(&self) -> TokenStream2 {
        let binding = &self.binding;
        quote! { #binding }
    }
    pub fn build_pat_ref(&self) -> TokenStream2 {
        let binding = &self.binding;
        quote! { ref #binding }
    }
    pub fn build_pat_mut(&self) -> TokenStream2 {
        let binding = &self.binding;
        quote! { ref mut #binding }
    }
    pub fn build_field_expr(&self) -> TokenStream2 {
        let label_type = build_label_type(&self.binding);
        let binding = &self.binding;
        let literal_name = self.binding.to_string();
        quote! { ::frunk_core::labelled::field_with_name::<#label_type, _>(#literal_name, #binding) }
    }
    pub fn build_field_pat(&self) -> TokenStream2 {
        let binding = &self.binding;
        quote! { ::frunk_core::labelled::Field { value: #binding, .. } }
    }
}

/// Represents the binding of a struct or enum variant's fields to a corresponding
/// set of similarly named local variables.
pub struct FieldBindings {
    pub type_: StructType,
    pub fields: Vec<FieldBinding>,
}

impl FieldBindings {
    pub fn new(fields: &Fields) -> Self {
        Self {
            type_: match fields {
                Fields::Named(_) => StructType::Named,
                Fields::Unnamed(_) => StructType::Tuple,
                Fields::Unit => StructType::Unit,
            },
            fields: fields
                .iter()
                .enumerate()
                .map(|(index, field)| FieldBinding {
                    field: field.clone(),
                    binding: field
                        .ident
                        .clone()
                        .unwrap_or_else(|| Ident::new(&format!("_{}", index), field.span())),
                })
                .collect(),
        }
    }

    /// Builds a type constructor for use with structs or enum variants. Does not include the name
    /// of the type or variant.
    pub fn build_type_constr<R: ToTokens>(&self, f: impl Fn(&FieldBinding) -> R) -> TokenStream2 {
        let bindings: Vec<_> = self.fields.iter().map(f).collect();
        match self.type_ {
            StructType::Named => quote! { { #(#bindings,)* } },
            StructType::Tuple => quote! { ( #(#bindings,)* ) },
            StructType::Unit => TokenStream2::new(),
        }
    }

    pub fn build_hlist_type<R: ToTokens>(&self, f: impl Fn(&FieldBinding) -> R) -> TokenStream2 {
        build_hlist_type(self.fields.iter().map(f))
    }

    pub fn build_hlist_constr<R: ToTokens>(&self, f: impl Fn(&FieldBinding) -> R) -> TokenStream2 {
        build_hlist_constr(self.fields.iter().map(f))
    }
}

pub fn ref_generics(generics: &Generics) -> Generics {
    let mut generics_ref = generics.clone();

    // instantiate a lifetime and lifetime def to add
    let ref_lifetime = Lifetime::new("'_frunk_ref_", Span::call_site());
    let ref_lifetime_def = LifetimeDef::new(ref_lifetime.clone());

    // Constrain the generic lifetimes present in the concrete type to the reference lifetime
    // of our implementation of LabelledGeneric for the reference case (& and &mut)
    {
        let generics_ref_lifetimes_mut = generics_ref.lifetimes_mut();
        for existing_lifetime_mut in generics_ref_lifetimes_mut {
            existing_lifetime_mut.bounds.push(ref_lifetime.clone());
        }
    }

    // Add our current generic lifetime to the list of generics
    let ref_lifetime_generic = GenericParam::Lifetime(ref_lifetime_def);
    generics_ref.params.push(ref_lifetime_generic);

    generics_ref
}
