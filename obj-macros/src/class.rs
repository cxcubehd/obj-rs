//! `#[obj::class]` — the data half of a class declaration.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Field, Fields, Ident, ItemStruct, Token};

use crate::common::*;

/// Arguments to `#[obj::class(...)]`.
pub struct ClassArgs {
    /// Base classes, primary first. The primary non-virtual base sits at offset 0 and is the
    /// `Deref` target.
    pub bases: Vec<BaseRef>,
    /// Whether this class has pure virtual methods and so cannot be instantiated.
    pub is_abstract: bool,
    /// Standard traits to carry on this class's interface, from `dyn_traits(..)`.
    pub dyn_traits: Vec<Ident>,
}

impl Parse for ClassArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut bases = Vec::new();
        let mut is_abstract = false;
        let mut dyn_traits: Vec<Ident> = Vec::new();
        while !input.is_empty() {
            let look = input.lookahead1();
            if look.peek(Token![abstract]) {
                input.parse::<Token![abstract]>()?;
                is_abstract = true;
            } else if look.peek(Ident) {
                let key: Ident = input.parse()?;
                if key == "extends" {
                    if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        bases.push(input.parse()?);
                    } else if input.peek(syn::token::Paren) {
                        let inner;
                        syn::parenthesized!(inner in input);
                        let listed = Punctuated::<BaseRef, Token![,]>::parse_terminated(&inner)?;
                        bases.extend(listed);
                    } else {
                        return Err(syn::Error::new(
                            key.span(),
                            "obj: write `extends = Base`, `extends(Primary, Secondary, ..)` or \
                             `extends(virtual Shared, ..)`",
                        ));
                    }
                } else if key == "dyn_traits" {
                    if !input.peek(syn::token::Paren) {
                        return Err(syn::Error::new(
                            key.span(),
                            "obj: write `dyn_traits(Debug, Clone, ..)`",
                        ));
                    }
                    let inner;
                    syn::parenthesized!(inner in input);
                    let listed = Punctuated::<Ident, Token![,]>::parse_terminated(&inner)?;
                    for t in listed {
                        if !DYN_TRAITS.contains(&t.to_string().as_str()) {
                            return Err(syn::Error::new(
                                t.span(),
                                format!(
                                    "obj: `{t}` is not one of the traits `dyn_traits` can carry \
                                     through a handle; expected one of {}",
                                    DYN_TRAITS.join(", "),
                                ),
                            ));
                        }
                        if dyn_traits.contains(&t) {
                            return Err(syn::Error::new(
                                t.span(),
                                "obj: duplicate `dyn_traits` entry",
                            ));
                        }
                        dyn_traits.push(t);
                    }
                } else {
                    return Err(syn::Error::new(
                        key.span(),
                        "obj: expected `extends = Base`, `extends(..)`, `abstract` or \
                         `dyn_traits(..)`",
                    ));
                }
            } else {
                return Err(look.error());
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        let mut seen: Vec<String> = Vec::new();
        for b in &bases {
            if seen.contains(&b.class.to_string()) {
                return Err(syn::Error::new(
                    b.class.span(),
                    "obj: duplicate base class; a class may be inherited virtually or directly, \
                     but not both",
                ));
            }
            seen.push(b.class.to_string());
        }

        // `Eq` is a marker on top of `PartialEq`, exactly as in `core`, so asking for it alone
        // would generate an impl whose supertrait is unsatisfied far from here.
        if let Some(eq) = dyn_traits.iter().find(|t| *t == "Eq") {
            if !dyn_traits.iter().any(|t| t == "PartialEq") {
                return Err(syn::Error::new(
                    eq.span(),
                    "obj: `dyn_traits(Eq)` also needs `PartialEq`, since `Eq` only marks an \
                     existing `PartialEq` as total",
                ));
            }
        }

        // Emit in a fixed order so the generated code does not depend on how the list was written.
        sort_dyn_traits(&mut dyn_traits);

        Ok(ClassArgs {
            bases,
            is_abstract,
            dyn_traits,
        })
    }
}

pub fn expand(args: ClassArgs, mut item: ItemStruct) -> syn::Result<TokenStream> {
    let class = item.ident.clone();
    let vis = item.vis.clone();

    let (Fields::Named(_) | Fields::Unit) = item.fields else {
        return Err(syn::Error::new_spanned(
            &item.fields,
            "obj: a class must have named fields",
        ));
    };

    let (direct, virt): (Vec<&BaseRef>, Vec<&BaseRef>) =
        args.bases.iter().partition(|b| !b.is_virtual);

    // Prefix layout: non-virtual base subobjects come first, primary at offset 0, so `&Derived` is
    // also a valid `&PrimaryBase`. A virtual base is *not* stored here at all — only a link to
    // wherever the complete object put the single shared copy.
    if !args.bases.is_empty() {
        if matches!(item.fields, Fields::Unit) {
            item.fields = Fields::Named(syn::parse_quote!({}));
        }
        let Fields::Named(named) = &mut item.fields else {
            unreachable!("normalised above")
        };
        for base in virt.iter().rev() {
            let (f, b) = (base_field(&base.class), &base.class);
            let doc = format!(
                "Link to the shared `{b}` base subobject.\n\nWrite \
                 [`VBase::new()`](obj::VBase::new) here; `{class}::complete(..)` is what links it \
                 to the one copy the complete object holds.",
            );
            let field: Field = syn::parse_quote!(#[doc = #doc] #vis #f: ::obj::VBase<#b>);
            named.named.insert(0, field);
        }
        for base in direct.iter().rev() {
            let (f, b) = (base_field(&base.class), &base.class);
            let doc = format!("The `{b}` base subobject.");
            let field: Field = syn::parse_quote!(#[doc = #doc] #vis #f: #b);
            named.named.insert(0, field);
        }
    }
    item.attrs.insert(0, syn::parse_quote!(#[repr(C)]));

    let ancestors_mac = ancestors_macro(&class);
    let is_abstract = args.is_abstract;
    let base_list = &args.bases;
    let dyn_traits = &args.dyn_traits;

    // Each class contributes its own entry to a list built by walking up the chain. Secondary
    // bases are queued as `pending`; the emitter resumes the walk through each of them in turn.
    let ancestors_def = match args.bases.split_first() {
        Some((primary, secondaries)) => {
            let primary_mac = ancestors_macro(&primary.class);
            quote! {
                #[doc(hidden)]
                #[macro_export]
                macro_rules! #ancestors_mac {
                    ({$($pre:tt)*} [$($pend:tt)*] [$($acc:tt)*]) => {
                        #primary_mac! {
                            {$($pre)*}
                            [$($pend)* #(#secondaries)*]
                            [$($acc)* (#class #is_abstract #vis [#(#base_list)*] [#(#dyn_traits)*])]
                        }
                    };
                }
            }
        }
        None => quote! {
            #[doc(hidden)]
            #[macro_export]
            macro_rules! #ancestors_mac {
                ({$($pre:tt)*} [$($pend:tt)*] [$($acc:tt)*]) => {
                    ::obj::__obj_emit! {
                        $($pre)*
                        pending [$($pend)*]
                        ancestors [$($acc)* (#class #is_abstract #vis [] [#(#dyn_traits)*])]
                    }
                };
            }
        },
    };

    // `Deref` targets the primary non-virtual base, which sits at offset 0. A class whose only
    // bases are virtual has nothing at offset 0, so it derefs to its first shared base instead —
    // one indirection rather than none, but it keeps `html.id` reaching through to `Doc` the way a
    // C++ user expects, and it is what lets inherited method lookup walk into the shared base.
    let primary_deref = match (direct.first(), virt.first()) {
        (Some(base), _) => {
            let (f, b) = (base_field(&base.class), &base.class);
            Some(quote! {
                impl ::obj::__private::Deref for #class {
                    type Target = #b;
                    #[inline]
                    fn deref(&self) -> &#b { &self.#f }
                }
                impl ::obj::__private::DerefMut for #class {
                    #[inline]
                    fn deref_mut(&mut self) -> &mut #b { &mut self.#f }
                }
                const _: () = assert!(
                    ::obj::__private::offset_of!(#class, #f) == 0,
                    "obj: the primary base subobject must be at offset 0",
                );
            })
        }
        (None, Some(base)) => {
            let (f, b) = (base_field(&base.class), &base.class);
            Some(quote! {
                impl ::obj::__private::Deref for #class {
                    type Target = #b;
                    #[inline]
                    fn deref(&self) -> &#b { self.#f.resolve(self) }
                }
                impl ::obj::__private::DerefMut for #class {
                    #[inline]
                    fn deref_mut(&mut self) -> &mut #b {
                        let link = self.#f.raw();
                        link.resolve_mut(self)
                    }
                }
            })
        }
        (None, None) => None,
    };

    // Secondary bases are not `Deref` targets, and neither is a virtual base past the first, so
    // both get named accessors.
    let secondary_accessors = direct.iter().skip(1).map(|base| {
        let (f, b) = (base_field(&base.class), &base.class);
        let getter = base_accessor(b);
        let getter_mut = base_accessor_mut(b);
        let doc = format!("Borrows the `{b}` base subobject.");
        let doc_mut = format!("Mutably borrows the `{b}` base subobject.");
        quote! {
            impl #class {
                #[doc = #doc]
                #[inline]
                #vis fn #getter(&self) -> &#b { &self.#f }
                #[doc = #doc_mut]
                #[inline]
                #vis fn #getter_mut(&mut self) -> &mut #b { &mut self.#f }
            }
        }
    });

    let virtual_accessors = virt.iter().map(|base| {
        let (f, b) = (base_field(&base.class), &base.class);
        let getter = base_accessor(b);
        let getter_mut = base_accessor_mut(b);
        let doc = format!(
            "Borrows the shared `{b}` base subobject.\n\nResolves the link stored in this \
             subobject, so it works from a plain `&{class}` without knowing the most-derived \
             class.\n\n# Panics\n\nIf this `{class}` was not built through a \
             `complete(..)` constructor, leaving the link unset.",
        );
        let doc_mut = format!("Mutably borrows the shared `{b}` base subobject.\n\n# Panics\n\nSee [`{class}::{getter}`].");
        quote! {
            impl #class {
                #[doc = #doc]
                #[inline]
                #vis fn #getter(&self) -> &#b { self.#f.resolve(self) }
                #[doc = #doc_mut]
                #[inline]
                #vis fn #getter_mut(&mut self) -> &mut #b {
                    let link = self.#f.raw();
                    link.resolve_mut(self)
                }
            }
        }
    });

    Ok(quote! {
        #item
        #primary_deref
        #(#secondary_accessors)*
        #(#virtual_accessors)*
        #ancestors_def

        // Walk the chain, then emit everything that needs the full ancestor list.
        #ancestors_mac! { {class #class bases [#(#base_list)*]} [] [] }
    })
}
