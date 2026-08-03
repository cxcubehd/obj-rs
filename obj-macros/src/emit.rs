//! The tail of both macros: everything that needs the *full* ancestor list.
//!
//! Neither attribute macro can see past its own item, so a class knows only its direct bases. The
//! `__obj_ancestors_*` chain of `macro_rules` walks up the hierarchy accumulating entries and
//! hands the complete list here.
//!
//! With multiple inheritance the walk has to branch. It stays a straight line by queueing
//! secondary bases in `pending`: when a root ends one branch, this macro re-enters the chain at
//! the next queued base, carrying the accumulated list along. Emission happens only once the
//! queue is empty.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitBool, LitStr, Token, TraitItemFn, Visibility};

use crate::common::*;

/// One entry of the accumulated list: `(Class is_abstract vis [DirectBases..])`.
pub struct Ancestor {
    pub class: Ident,
    pub is_abstract: bool,
    pub vis: Visibility,
    pub direct_bases: Vec<Ident>,
}

impl Parse for Ancestor {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let class = content.parse()?;
        let is_abstract = content.parse::<LitBool>()?.value;
        let vis = content.parse()?;
        let bases;
        syn::bracketed!(bases in content);
        let mut direct_bases = Vec::new();
        while !bases.is_empty() {
            direct_bases.push(bases.parse()?);
        }
        Ok(Ancestor {
            class,
            is_abstract,
            vis,
            direct_bases,
        })
    }
}

pub enum Kind {
    Class {
        bases: Vec<Ident>,
    },
    Methods {
        virtual_sigs: TokenStream,
        provides: Vec<Ident>,
    },
}

/// How a class reaches the implementation of a method it does not provide itself.
pub struct Delegate {
    /// The base to hand the call to.
    pub class: Ident,
    /// The field holding that base, or `None` when the class implements its own interface.
    pub field: Option<Ident>,
}

impl Parse for Delegate {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let class = content.parse()?;
        let field = if content.peek(Token![self]) {
            content.parse::<Token![self]>()?;
            None
        } else {
            Some(content.parse()?)
        };
        Ok(Delegate { class, field })
    }
}

/// One `impl <Owner>Dyn for <Target>` block.
pub struct IfaceInput {
    pub owner: Ident,
    pub target: Ident,
    pub delegate: Delegate,
    pub overrides: Vec<Ident>,
    pub methods: Vec<TraitItemFn>,
}

pub enum EmitInput {
    Walk {
        kind: Kind,
        class: Ident,
        pending: Vec<Ident>,
        ancestors: Vec<Ancestor>,
    },
    Iface(IfaceInput),
}

fn expect_ident(input: ParseStream, want: &str) -> syn::Result<()> {
    let got: Ident = input.parse()?;
    if got != want {
        return Err(syn::Error::new(got.span(), "obj: internal macro misuse"));
    }
    Ok(())
}

impl Parse for EmitInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let kind_tok: Ident = input.parse()?;

        if kind_tok == "iface" {
            let owner: Ident = input.parse()?;
            input.parse::<Token![for]>()?;
            let target: Ident = input.parse()?;
            expect_ident(input, "delegate")?;
            let delegate: Delegate = input.parse()?;
            expect_ident(input, "overrides")?;
            let list;
            syn::bracketed!(list in input);
            let mut overrides = Vec::new();
            while !list.is_empty() {
                overrides.push(list.parse()?);
            }
            expect_ident(input, "methods")?;
            let body;
            syn::braced!(body in input);
            let mut methods = Vec::new();
            while !body.is_empty() {
                methods.push(body.parse()?);
            }
            return Ok(EmitInput::Iface(IfaceInput {
                owner,
                target,
                delegate,
                overrides,
                methods,
            }));
        }

        let class: Ident = input.parse()?;

        let kind = if kind_tok == "class" {
            expect_ident(input, "bases")?;
            let list;
            syn::bracketed!(list in input);
            let mut bases = Vec::new();
            while !list.is_empty() {
                bases.push(list.parse()?);
            }
            Kind::Class { bases }
        } else if kind_tok == "methods" {
            let content;
            syn::braced!(content in input);
            let virtual_sigs = content.parse()?;
            expect_ident(input, "provides")?;
            let list;
            syn::bracketed!(list in input);
            let mut provides = Vec::new();
            while !list.is_empty() {
                provides.push(list.parse()?);
            }
            Kind::Methods {
                virtual_sigs,
                provides,
            }
        } else {
            return Err(syn::Error::new(
                kind_tok.span(),
                "obj: internal macro misuse",
            ));
        };

        expect_ident(input, "pending")?;
        let queue;
        syn::bracketed!(queue in input);
        let mut pending = Vec::new();
        while !queue.is_empty() {
            pending.push(queue.parse()?);
        }

        expect_ident(input, "ancestors")?;
        let list;
        syn::bracketed!(list in input);
        let mut ancestors = Vec::new();
        while !list.is_empty() {
            ancestors.push(list.parse()?);
        }

        Ok(EmitInput::Walk {
            kind,
            class,
            pending,
            ancestors,
        })
    }
}

pub fn expand(input: EmitInput) -> syn::Result<TokenStream> {
    let (kind, class, pending, ancestors_raw) = match input {
        EmitInput::Iface(i) => return expand_iface(&i),
        EmitInput::Walk {
            kind,
            class,
            pending,
            ancestors,
        } => (kind, class, pending, ancestors),
    };

    // Branches of the hierarchy still to walk: re-enter the chain at the next one.
    if let Some((next, rest)) = pending.split_first() {
        let next_mac = ancestors_macro(next);
        let prefix = match &kind {
            Kind::Class { bases } => quote!(class #class bases [#(#bases)*]),
            Kind::Methods {
                virtual_sigs,
                provides,
            } => quote!(methods #class { #virtual_sigs } provides [#(#provides)*]),
        };
        let acc = ancestors_raw.iter().map(|a| {
            let (c, ab, v, b) = (&a.class, a.is_abstract, &a.vis, &a.direct_bases);
            quote!((#c #ab #v [#(#b)*]))
        });
        return Ok(quote! {
            #next_mac! { {#prefix} [#(#rest)*] [#(#acc)*] }
        });
    }

    // A class reachable through two branches appears twice; keep the first, which is the one
    // reached along the primary chain.
    let mut seen: Vec<String> = Vec::new();
    let ancestors: Vec<&Ancestor> = ancestors_raw
        .iter()
        .filter(|a| {
            let name = a.class.to_string();
            if seen.contains(&name) {
                false
            } else {
                seen.push(name);
                true
            }
        })
        .collect();

    match &kind {
        Kind::Class { bases } => expand_class(&class, bases, &ancestors),
        Kind::Methods {
            virtual_sigs,
            provides,
        } => expand_methods(&class, virtual_sigs, provides, &ancestors),
    }
}

/// Emits `impl <Owner>Dyn for <Target>`, choosing per method between the target's own inherent
/// override and a hand-off to the base that provides it.
fn expand_iface(input: &IfaceInput) -> syn::Result<TokenStream> {
    let IfaceInput {
        owner,
        target,
        delegate,
        overrides,
        methods,
    } = input;
    let iface = iface_trait(owner);
    let owner_iface = iface_trait(owner);

    let bodies = methods.iter().map(|m| {
        let sig = &m.sig;
        let name = &sig.ident;
        let args: Vec<&Ident> = sig
            .inputs
            .iter()
            .filter_map(|a| match a {
                syn::FnArg::Typed(t) => match &*t.pat {
                    syn::Pat::Ident(p) => Some(&p.ident),
                    _ => None,
                },
                syn::FnArg::Receiver(_) => None,
            })
            .collect();

        let call = if overrides.iter().any(|o| o == name) {
            // The target implements this itself.
            quote!(#target::#name(self #(, #args)*))
        } else {
            match &delegate.field {
                // Implementing its own interface: the inherent method must exist, and its
                // absence is exactly the "pure virtual never overridden" error.
                None => quote!(#target::#name(self #(, #args)*)),
                Some(field) => {
                    let mutability = sig
                        .receiver()
                        .and_then(|r| r.mutability)
                        .map(|_| quote!(mut));
                    let base = &delegate.class;
                    if base == owner {
                        // The provider is the interface's own class: call its inherent body.
                        quote!(#owner::#name(& #mutability self.#field #(, #args)*))
                    } else {
                        // An intermediate base: go through its interface impl, which resolves
                        // the same question one level up.
                        quote!(
                            <#base as #owner_iface>::#name(& #mutability self.#field #(, #args)*)
                        )
                    }
                }
            }
        };
        quote! {
            #[inline]
            #sig { #call }
        }
    });

    Ok(quote! {
        impl #iface for #target {
            #(#bodies)*
        }
    })
}

fn self_entry(class: &Ident, is_abstract: bool) -> TokenStream {
    let iface = iface_trait(class);
    let vtable = if is_abstract {
        // An abstract class implements no interface, not even its own, so there is no vtable to
        // record. Its table is never the one a cast consults, because it can never be the
        // most-derived class of a live object.
        quote!(::core::option::Option::None)
    } else {
        quote!(::core::option::Option::Some(
            ::obj::__vtable_of!(#class as dyn #iface)
        ))
    };
    quote! {
        ::obj::BaseEntry {
            id: ::obj::__private::TypeId::of::<#class>,
            data_offset: 0,
            dyn_vtable: #vtable,
        }
    }
}

fn expand_class(
    class: &Ident,
    bases: &[Ident],
    ancestors: &[&Ancestor],
) -> syn::Result<TokenStream> {
    let me = ancestors
        .first()
        .ok_or_else(|| syn::Error::new(class.span(), "obj: empty ancestor list"))?;
    let vis = &me.vis;
    let is_abstract = me.is_abstract;

    let iface = iface_trait(class);
    let sub_tr = sub_trait(class);
    let bases_tr = bases_trait(class);
    let table = table_static(class);
    let meta = meta_static(class);
    let sub_fn = sub_fn(class);
    let sub_fn_mut = sub_fn_mut(class);
    let name_lit = LitStr::new(&class.to_string(), class.span());
    let entry = self_entry(class, is_abstract);

    // One sub-table per base, each shifted by where that base sits inside this class. Concrete
    // classes go through the base's `__ObjBases*` trait, which rewrites every inherited entry to
    // carry *this* class's vtables.
    let branch = |b: &Ident| {
        let f = base_field(b);
        let b_bases = bases_trait(b);
        if is_abstract {
            quote! {
                ::obj::BaseTable::from_slice_without_vtables(<#b as ::obj::Class>::META.bases)
                    .offset_by(::obj::__private::offset_of!(#class, #f))
            }
        } else {
            quote! {
                <#class as #b_bases>::OBJ_TABLE
                    .offset_by(::obj::__private::offset_of!(#class, #f))
            }
        }
    };

    let table_expr = match bases.split_first() {
        Some((primary, secondaries)) => {
            let head = branch(primary);
            let rest = secondaries.iter().map(|b| {
                let t = branch(b);
                quote!(.concat(#t))
            });
            quote!(#head #(#rest)* .push(#entry))
        }
        None => quote!(::obj::BaseTable::EMPTY.push(#entry)),
    };

    // The same construction for an arbitrary subclass `C`, so descendants inherit entries for
    // ancestors they were never told about.
    let bases_impl = {
        let extra_bounds = bases.iter().map(|b| {
            let t = bases_trait(b);
            quote!(+ #t)
        });
        let sub_entry = quote! {
            ::obj::BaseEntry {
                id: ::obj::__private::TypeId::of::<#class>,
                data_offset: 0,
                dyn_vtable: ::core::option::Option::Some(::obj::__vtable_of!(C as dyn #iface)),
            }
        };
        let expr = match bases.split_first() {
            Some((primary, secondaries)) => {
                let mk = |b: &Ident| {
                    let f = base_field(b);
                    let t = bases_trait(b);
                    quote! {
                        <C as #t>::OBJ_TABLE
                            .offset_by(::obj::__private::offset_of!(#class, #f))
                    }
                };
                let head = mk(primary);
                let rest = secondaries.iter().map(|b| {
                    let t = mk(b);
                    quote!(.concat(#t))
                });
                quote!(#head #(#rest)* .push(#sub_entry))
            }
            None => quote!(::obj::BaseTable::EMPTY.push(#sub_entry)),
        };
        quote! {
            impl<C> #bases_tr for C
            where
                C: ::obj::Class + #iface + Sized + ::obj::SubclassOf<#class> #(#extra_bounds)*,
            {
                const OBJ_TABLE: ::obj::BaseTable = #expr;
            }
        }
    };

    // Upcasts to every ancestor, each one a plain trait-upcasting coercion.
    let upcasts = ancestors.iter().map(|a| {
        let anc = &a.class;
        let anc_iface = iface_trait(anc);
        quote! {
            unsafe impl ::obj::SubclassOf<#anc> for #class {
                #[inline]
                fn up_box(
                    this: ::obj::__private::Box<dyn #iface>,
                ) -> ::obj::__private::Box<dyn #anc_iface> { this }
                #[inline]
                fn up_ref<'a>(this: &'a (dyn #iface + 'static)) -> &'a (dyn #anc_iface + 'static) {
                    this
                }
                #[inline]
                fn up_mut<'a>(
                    this: &'a mut (dyn #iface + 'static),
                ) -> &'a mut (dyn #anc_iface + 'static) { this }
            }
        }
    });

    let concrete = (!is_abstract).then(|| {
        quote! {
            unsafe impl ::obj::Concrete for #class {
                #[inline]
                fn into_dyn(
                    value: ::obj::__private::Box<#class>,
                ) -> ::obj::__private::Box<dyn #iface> { value }
                #[inline]
                fn as_dyn(value: &#class) -> &(dyn #iface + 'static) { value }
                #[inline]
                fn as_dyn_mut(value: &mut #class) -> &mut (dyn #iface + 'static) { value }
            }
        }
    });

    Ok(quote! {
        // Locates this class's subobject inside any object that derives from it. Resolving the
        // offset at runtime is what frees a class from knowing the layout of its descendants.
        #[doc(hidden)]
        #vis trait #sub_tr {
            fn #sub_fn(&self) -> &#class;
            fn #sub_fn_mut(&mut self) -> &mut #class;
        }

        impl<T: ::obj::AnyObj + ?Sized> #sub_tr for T {
            #[inline]
            fn #sub_fn(&self) -> &#class {
                ::obj::__private::subobject::<#class, T>(self)
            }
            #[inline]
            fn #sub_fn_mut(&mut self) -> &mut #class {
                ::obj::__private::subobject_mut::<#class, T>(self)
            }
        }

        // Field access through any polymorphic handle.
        impl ::obj::__private::Deref for dyn #iface {
            type Target = #class;
            #[inline]
            fn deref(&self) -> &#class { self.#sub_fn() }
        }
        impl ::obj::__private::DerefMut for dyn #iface {
            #[inline]
            fn deref_mut(&mut self) -> &mut #class { self.#sub_fn_mut() }
        }

        unsafe impl ::obj::AnyObj for #class {
            #[inline]
            fn class_meta(&self) -> &'static ::obj::ClassMeta { &#meta }
            #[inline]
            fn obj_addr(&self) -> *const u8 { (self as *const Self).cast::<u8>() }
        }

        #[doc(hidden)]
        #vis static #table: ::obj::BaseTable = #table_expr;

        #[doc(hidden)]
        #vis static #meta: ::obj::ClassMeta = ::obj::ClassMeta {
            name: #name_lit,
            id: ::obj::__private::TypeId::of::<#class>,
            bases: #table.as_slice(),
        };

        unsafe impl ::obj::Class for #class {
            type Dyn = dyn #iface;
            type Complete = #class;
            const META: &'static ::obj::ClassMeta = &#meta;
        }

        #[doc(hidden)]
        #vis trait #bases_tr: ::obj::Class + #iface + Sized {
            const OBJ_TABLE: ::obj::BaseTable;
        }
        #bases_impl

        #(#upcasts)*
        #concrete
    })
}

/// Walks the class graph to find whether `from` is, or derives from, `target`.
fn reaches(graph: &[(&Ident, &Vec<Ident>)], from: &Ident, target: &Ident) -> bool {
    if from == target {
        return true;
    }
    graph
        .iter()
        .find(|(name, _)| *name == from)
        .is_some_and(|(_, bases)| bases.iter().any(|b| reaches(graph, b, target)))
}

fn expand_methods(
    class: &Ident,
    virtual_sigs: &TokenStream,
    provides: &[Ident],
    ancestors: &[&Ancestor],
) -> syn::Result<TokenStream> {
    let me = ancestors
        .first()
        .ok_or_else(|| syn::Error::new(class.span(), "obj: empty ancestor list"))?;
    let vis = &me.vis;
    let iface = iface_trait(class);
    let sub_tr = sub_trait(class);

    // The interface's supertraits mirror the class's bases -- every base, not just the primary --
    // so `dyn Derived` upcasts to `dyn Base` for free along any branch.
    let supertraits = if me.direct_bases.is_empty() {
        quote!(::obj::AnyObj)
    } else {
        let each = me.direct_bases.iter().map(iface_trait);
        quote!(#(#each)+*)
    };

    let graph: Vec<(&Ident, &Vec<Ident>)> = ancestors
        .iter()
        .map(|a| (&a.class, &a.direct_bases))
        .collect();

    // One interface impl per ancestor. Each names the base that non-overridden methods are
    // delegated to, which is the direct base through which this class reaches that ancestor.
    let iface_impls = if me.is_abstract {
        None
    } else {
        let mut calls = Vec::new();
        for a in ancestors {
            let owner = &a.class;
            let mac = iface_macro(owner);
            let delegate = if owner == class {
                quote!((#class self))
            } else {
                let Some(via) = me.direct_bases.iter().find(|d| reaches(&graph, d, owner)) else {
                    return Err(syn::Error::new(
                        class.span(),
                        format!("obj: no base of `{class}` reaches `{owner}`"),
                    ));
                };
                let field = base_field(via);
                quote!((#via #field))
            };
            calls.push(quote!(#mac! { #class, #delegate, [#(#provides)*] }));
        }
        Some(quote!(#(#calls)*))
    };

    Ok(quote! {
        #vis trait #iface: #supertraits + #sub_tr {
            #virtual_sigs
        }

        #iface_impls
    })
}
