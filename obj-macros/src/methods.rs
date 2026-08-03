//! `#[obj::methods]` — the behaviour half of a class declaration.
//!
//! Every method becomes an inherent method on the class, which is what a `super` call resolves to
//! (`Shape::scale(self, k)`) and what a static `&Circle` view dispatches to.
//!
//! Virtual dispatch is then wired up by *delegation* rather than by name lookup: for each ancestor
//! interface, a class emits one impl in which every method either calls this class's own inherent
//! override or hands off to the base that provides it.
//!
//! Resolving this structurally matters twice over. Autoderef only ever walks the primary base
//! chain, so a lookup-based scheme can never reach a secondary base's methods; and delegation
//! bottoms out at an inherent method that does not exist when a pure virtual was never overridden,
//! turning that mistake into a compile error.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, Pat, Token, TraitItemFn};

use crate::common::*;

pub struct MethodsInput {
    pub self_ty: Ident,
    pub items: Vec<TraitItemFn>,
}

impl Parse for MethodsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        input.parse::<Token![impl]>()?;
        let self_ty: Ident = input.parse()?;
        let content;
        syn::braced!(content in input);
        let mut items = Vec::new();
        while !content.is_empty() {
            items.push(content.parse()?);
        }
        Ok(MethodsInput { self_ty, items })
    }
}

/// How a method participates in dispatch.
#[derive(PartialEq, Clone, Copy)]
enum Kind {
    /// Ordinary method: inherited through `Deref`, never dispatched virtually.
    Plain,
    /// Declares a new virtual method on this class's interface.
    Virtual,
    /// Reimplements a virtual declared by an ancestor.
    Override,
}

fn classify(f: &TraitItemFn) -> syn::Result<Kind> {
    let mut kind = Kind::Plain;
    for attr in &f.attrs {
        if !attr.path().is_ident("obj") {
            continue;
        }
        attr.parse_args_with(|input: ParseStream| {
            let look = input.lookahead1();
            if look.peek(Token![virtual]) {
                input.parse::<Token![virtual]>()?;
                kind = Kind::Virtual;
            } else if look.peek(Token![override]) {
                input.parse::<Token![override]>()?;
                // `override(Base)` is accepted and ignored: the declaring class is found
                // structurally, so naming it is documentation only.
                if input.peek(syn::token::Paren) {
                    let inner;
                    syn::parenthesized!(inner in input);
                    let _ = inner.parse::<TokenStream>()?;
                }
                kind = Kind::Override;
            } else {
                return Err(look.error());
            }
            Ok(())
        })?;
    }
    Ok(kind)
}

/// Strips `#[obj(..)]` markers, leaving doc comments and other attributes intact.
fn strip_obj_attrs(f: &mut TraitItemFn) {
    f.attrs.retain(|a| !a.path().is_ident("obj"));
}

fn check_dispatchable(f: &TraitItemFn) -> syn::Result<()> {
    let Some(receiver) = f.sig.receiver() else {
        return Err(syn::Error::new(
            f.sig.span(),
            "obj: a virtual method needs a `self` receiver",
        ));
    };
    if receiver.reference.is_none() {
        return Err(syn::Error::new(
            receiver.span(),
            "obj: a virtual method must take `&self` or `&mut self`, not `self` by value",
        ));
    }
    if !f.sig.generics.params.is_empty() {
        return Err(syn::Error::new(
            f.sig.generics.span(),
            "obj: virtual methods cannot be generic, because the interface must stay \
             object-safe; consider a non-virtual method instead",
        ));
    }
    if f.sig.asyncness.is_some() {
        return Err(syn::Error::new(
            f.sig.span(),
            "obj: virtual methods cannot be `async`, because the interface must stay object-safe",
        ));
    }
    for arg in &f.sig.inputs {
        if let FnArg::Typed(t) = arg {
            if !matches!(&*t.pat, Pat::Ident(_)) {
                return Err(syn::Error::new(
                    t.pat.span(),
                    "obj: virtual methods need plain identifier parameters",
                ));
            }
        }
    }
    Ok(())
}

pub fn expand(input: MethodsInput) -> syn::Result<TokenStream> {
    let class = input.self_ty;
    let iface_mac = iface_macro(&class);
    let ancestors_mac = ancestors_macro(&class);

    let mut inherent = Vec::new();
    let mut virtual_sigs = Vec::new();
    // Methods this class implements itself: its own new virtuals plus anything it overrides.
    let mut provided = Vec::new();

    for item in input.items {
        let kind = classify(&item)?;
        let mut item = item;
        strip_obj_attrs(&mut item);

        if kind != Kind::Plain {
            check_dispatchable(&item)?;
        }

        let sig = item.sig.clone();
        let attrs = item.attrs.clone();

        match &item.default {
            Some(body) => inherent.push(quote! { #(#attrs)* pub #sig #body }),
            None if kind == Kind::Virtual => {
                // A pure virtual: deliberately no inherent method, so a concrete class that
                // never overrides it fails to compile.
            }
            None => {
                return Err(syn::Error::new(
                    sig.span(),
                    "obj: a method without a body must be marked `#[obj(virtual)]`",
                ))
            }
        }

        match kind {
            Kind::Virtual => {
                virtual_sigs.push(quote! { #sig ; });
                if item.default.is_some() {
                    provided.push(sig.ident.clone());
                }
            }
            Kind::Override => provided.push(sig.ident.clone()),
            Kind::Plain => {}
        }
    }

    Ok(quote! {
        impl #class {
            #(#inherent)*
        }

        // Implements this class's interface for some class `$d`. `$delegate` names the base that
        // non-overridden methods are handed to; `$ov` lists what `$d` implements itself.
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #iface_mac {
            ($d:ident, $delegate:tt, [$($ov:ident)*]) => {
                ::obj::__obj_emit! {
                    iface #class for $d delegate $delegate overrides [$($ov)*]
                    methods { #(#virtual_sigs)* }
                }
            };
        }

        #ancestors_mac! {
            {methods #class { #(#virtual_sigs)* } provides [#(#provided)*]}
            []
            []
        }
    })
}
