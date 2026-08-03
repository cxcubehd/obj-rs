//! `#[obj::methods]` — the behaviour half of a class declaration.
//!
//! Dispatch works by forwarding each interface method to a *reserved* inherent name,
//! `__obj_own_<method>`, with method-call syntax. Autoderef climbs the base chain and stops at the
//! first class that defines it — the most-derived override — which is exactly C++ virtual
//! dispatch, and it means a class never needs to know which ancestor last overrode a method.
//!
//! The reserved name is essential. Forwarding to the public name instead (`self.area()`) resolves
//! to the *trait* method at the very first step, because trait methods are considered before
//! autoderef moves on, and the interface method would call itself forever. No trait declares
//! `__obj_own_*`, so that lookup can only ever find an inherent method.

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
#[derive(PartialEq)]
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
                // An optional `override(Base)` is accepted and ignored: dispatch resolves by
                // method-call syntax, so naming the declaring class is documentation only.
                if input.peek(syn::token::Paren) {
                    let _inner;
                    syn::parenthesized!(_inner in input);
                    let _ = _inner.parse::<TokenStream>()?;
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

/// `area` -> `__obj_own_area`, the reserved inherent name interface methods forward to.
fn own_name(name: &Ident) -> Ident {
    Ident::new(&format!("__obj_own_{name}"), name.span())
}

/// Strips `#[obj(..)]` markers, leaving doc comments and other attributes intact.
fn strip_obj_attrs(f: &mut TraitItemFn) {
    f.attrs.retain(|a| !a.path().is_ident("obj"));
}

/// The names of a method's parameters, for building a forwarding call.
fn arg_names(f: &TraitItemFn) -> syn::Result<Vec<Ident>> {
    let mut names = Vec::new();
    for arg in &f.sig.inputs {
        match arg {
            FnArg::Receiver(_) => {}
            FnArg::Typed(t) => match &*t.pat {
                Pat::Ident(p) => names.push(p.ident.clone()),
                other => {
                    return Err(syn::Error::new(
                        other.span(),
                        "obj: virtual methods need plain identifier parameters",
                    ))
                }
            },
        }
    }
    Ok(names)
}

fn check_dispatchable(f: &TraitItemFn) -> syn::Result<()> {
    if f.sig.receiver().is_none() {
        return Err(syn::Error::new(
            f.sig.span(),
            "obj: a virtual method needs a `self` receiver",
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
    Ok(())
}

pub fn expand(input: MethodsInput) -> syn::Result<TokenStream> {
    let class = input.self_ty;
    let iface = iface_trait(&class);
    let iface_mac = iface_macro(&class);
    let ancestors_mac = ancestors_macro(&class);

    let mut inherent = Vec::new();
    let mut virtual_sigs = Vec::new();
    let mut forwards = Vec::new();

    for item in input.items {
        let kind = classify(&item)?;
        let mut item = item;
        strip_obj_attrs(&mut item);

        if kind != Kind::Plain {
            check_dispatchable(&item)?;
        }

        let sig = item.sig.clone();
        let attrs = item.attrs.clone();
        let name = sig.ident.clone();

        if kind == Kind::Plain {
            let Some(body) = &item.default else {
                return Err(syn::Error::new(
                    sig.span(),
                    "obj: a method without a body must be marked `#[obj(virtual)]`",
                ));
            };
            inherent.push(quote! { #(#attrs)* pub #sig #body });
        } else {
            // The reserved name autoderef will search for. Only classes that declare or override
            // the method define it, so the search lands on the most-derived implementation.
            let own = own_name(&name);
            let mut own_sig = sig.clone();
            own_sig.ident = own.clone();
            let names = arg_names(&item)?;

            match &item.default {
                Some(body) => {
                    inherent.push(quote! { #(#attrs)* pub #sig #body });
                    inherent.push(quote! {
                        #[doc(hidden)]
                        #[inline]
                        pub #own_sig { Self::#name(self #(, #names)*) }
                    });
                }
                None => {
                    if kind != Kind::Virtual {
                        return Err(syn::Error::new(
                            sig.span(),
                            "obj: a method without a body must be marked `#[obj(virtual)]`",
                        ));
                    }
                    // A pure virtual. The placeholder is what a concrete subclass that forgets to
                    // override it will resolve to, turning silent infinite recursion into a
                    // deprecation warning plus a clear panic.
                    let msg = format!("obj: pure virtual `{class}::{name}` was never overridden");
                    let dep = format!(
                        "obj: pure virtual `{class}::{name}` has no override in this class",
                    );
                    inherent.push(quote! {
                        #(#attrs)*
                        #[doc(hidden)]
                        #[deprecated(note = #dep)]
                        pub #own_sig { ::core::panic!(#msg) }
                    });
                }
            }

            if kind == Kind::Virtual {
                virtual_sigs.push(quote! { #sig ; });
                forwards.push(quote! {
                    #[inline]
                    #sig { self.#own(#(#names),*) }
                });
            }
        }
    }

    Ok(quote! {
        impl #class {
            #(#inherent)*
        }

        // Implements this class's interface for any subclass.
        #[doc(hidden)]
        #[macro_export]
        macro_rules! #iface_mac {
            ($d:ty) => {
                impl #iface for $d {
                    #(#forwards)*
                }
            };
        }

        #ancestors_mac! { {methods #class { #(#virtual_sigs)* }} [] }
    })
}
