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

/// One entry of the accumulated list: `(Class is_abstract vis [DirectBases..] [DynTraits..])`.
pub struct Ancestor {
    pub class: Ident,
    pub is_abstract: bool,
    pub vis: Visibility,
    pub direct_bases: Vec<BaseRef>,
    pub dyn_traits: Vec<Ident>,
}

impl Ancestor {
    /// The bases stored inside this class's own layout.
    fn stored_bases(&self) -> impl Iterator<Item = &BaseRef> {
        self.direct_bases.iter().filter(|b| !b.is_virtual)
    }

    /// The bases this class shares with the rest of the complete object.
    fn virtual_bases(&self) -> impl Iterator<Item = &BaseRef> {
        self.direct_bases.iter().filter(|b| b.is_virtual)
    }
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
        let traits;
        syn::bracketed!(traits in content);
        let mut dyn_traits = Vec::new();
        while !traits.is_empty() {
            dyn_traits.push(traits.parse()?);
        }
        Ok(Ancestor {
            class,
            is_abstract,
            vis,
            direct_bases,
            dyn_traits,
        })
    }
}

/// The trait a `dyn_traits(..)` entry becomes a supertrait of the class's interface.
///
/// `Debug` and `Display` are object-safe already and pass straight through; the rest name their
/// object-safe shim from [`obj::dyn_traits`](../obj/dyn_traits/index.html).
fn dyn_trait_path(name: &Ident) -> Option<TokenStream> {
    Some(match name.to_string().as_str() {
        "Debug" => quote!(::core::fmt::Debug),
        "Display" => quote!(::core::fmt::Display),
        "Clone" => quote!(::obj::CloneObj),
        "PartialEq" => quote!(::obj::DynEq),
        "Eq" => quote!(::obj::DynTotalEq),
        "Hash" => quote!(::obj::DynHash),
        _ => return None,
    })
}

/// The impl that backs a `dyn_traits(..)` entry for a concrete class.
///
/// `ty` is the type that actually implements the class's interface, which is the class itself
/// today and its `Complete` wrapper once virtual bases are in play.
fn dyn_trait_impl(name: &Ident, ty: &TokenStream) -> Option<TokenStream> {
    Some(match name.to_string().as_str() {
        // Object-safe as they stand: the supertrait is the whole mechanism.
        "Debug" | "Display" => return None,
        "Clone" => quote! {
            // SAFETY: the clone is produced by `<#ty as Clone>::clone`, so it has exactly the
            // most-derived type of `self`, which is what `CloneObj` requires.
            unsafe impl ::obj::CloneObj for #ty {
                #[inline]
                fn clone_raw(&self) -> *mut u8 {
                    ::obj::__private::Box::into_raw(::obj::__private::Box::new(
                        <#ty as ::obj::__private::Clone>::clone(self),
                    ))
                    .cast::<u8>()
                }
            }
        },
        "PartialEq" => quote! {
            impl ::obj::DynEq for #ty {
                #[inline]
                fn as_any(&self) -> &dyn ::obj::__private::Any { self }
                #[inline]
                fn dyn_eq(&self, other: &dyn ::obj::__private::Any) -> bool {
                    match other.downcast_ref::<#ty>() {
                        ::core::option::Option::Some(o) => {
                            <#ty as ::obj::__private::PartialEq>::eq(self, o)
                        }
                        ::core::option::Option::None => false,
                    }
                }
            }
        },
        "Eq" => quote!(impl ::obj::DynTotalEq for #ty {}),
        "Hash" => quote! {
            impl ::obj::DynHash for #ty {
                #[inline]
                fn dyn_hash(&self, mut state: &mut dyn ::obj::__private::Hasher) {
                    // Fold in the class identity first, so two objects of different classes hash
                    // differently even when their fields agree -- matching `DynEq`, which never
                    // calls them equal.
                    ::obj::__private::Hash::hash(
                        &::obj::__private::TypeId::of::<#ty>(),
                        &mut state,
                    );
                    ::obj::__private::Hash::hash(self, &mut state);
                }
            }
        },
        _ => return None,
    })
}

/// Every `dyn_traits(..)` entry declared anywhere in this class's ancestry.
///
/// A subclass has to generate the shim impls for traits its *bases* opted into, because the
/// interface it implements carries them as supertraits.
fn inherited_dyn_traits(ancestors: &[&Ancestor]) -> Vec<Ident> {
    let mut out: Vec<Ident> = Vec::new();
    for a in ancestors {
        for t in &a.dyn_traits {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
    }
    sort_dyn_traits(&mut out);
    out
}

/// Where every subobject of a complete object lives.
///
/// Without virtual bases this is trivial: the class *is* the complete object, and every ancestor
/// is a field chain away. With them the shared bases cannot sit inside any one subobject — two
/// paths reach them and there must still be one copy — so the macro generates a wrapper holding
/// the class plus one copy of each shared base, and every offset is measured from that wrapper.
struct Layout {
    /// The type that owns a complete object: the class, or its generated wrapper.
    complete: Ident,
    /// Whether a wrapper was needed.
    wrapped: bool,
    /// The shared bases the wrapper stores, deduplicated, in a stable order.
    vbases: Vec<Ident>,
    /// Field path from `complete` down to each ancestor's subobject.
    paths: Vec<(Ident, TokenStream)>,
}

impl Layout {
    fn of(class: &Ident, is_abstract: bool, ancestors: &[&Ancestor]) -> syn::Result<Self> {
        let mut vbases: Vec<Ident> = Vec::new();
        for a in ancestors {
            for v in a.virtual_bases() {
                if !vbases.contains(&v.class) {
                    vbases.push(v.class.clone());
                }
            }
        }

        // Inheriting one class both ways would mean two copies under one name, and every lookup
        // would have to say which it meant. C++ permits it; refusing is clearer and costs nothing
        // anyone actually wants.
        for a in ancestors {
            for b in a.stored_bases() {
                if vbases.contains(&b.class) {
                    return Err(syn::Error::new(
                        b.class.span(),
                        format!(
                            "obj: `{}` is inherited both virtually and directly within `{class}`'s \
                             hierarchy; make every path to it `virtual`, or none of them",
                            b.class,
                        ),
                    ));
                }
            }
        }

        // An abstract class is never a complete object, so it stores no shared base and needs no
        // wrapper. Each concrete class below it holds the one copy instead.
        let wrapped = !vbases.is_empty() && !is_abstract;
        if !wrapped {
            vbases.clear();
        }
        let complete = if wrapped {
            complete_type(class)
        } else {
            class.clone()
        };

        // Roots: the class's own subobject, and one slot per shared base.
        let mut paths: Vec<(Ident, TokenStream)> = Vec::new();
        if wrapped {
            let f = base_field(class);
            paths.push((class.clone(), quote!(#f)));
            for v in &vbases {
                let f = base_field(v);
                paths.push((v.clone(), quote!(#f)));
            }
        } else {
            paths.push((class.clone(), TokenStream::new()));
        }

        // The ancestor list runs derived-before-base, so a single forward pass suffices: by the
        // time a class is visited, whichever branch reached it first has already given it a path.
        for a in ancestors {
            let Some(prefix) = paths
                .iter()
                .find(|(c, _)| *c == a.class)
                .map(|(_, p)| p.clone())
            else {
                continue;
            };
            for b in a.stored_bases() {
                if paths.iter().any(|(c, _)| *c == b.class) {
                    continue;
                }
                let f = base_field(&b.class);
                let path = if prefix.is_empty() {
                    quote!(#f)
                } else {
                    quote!(#prefix.#f)
                };
                paths.push((b.class.clone(), path));
            }
        }

        Ok(Layout {
            complete,
            wrapped,
            vbases,
            paths,
        })
    }

    /// The field path from the complete object to `class`'s subobject.
    fn path(&self, class: &Ident) -> Option<&TokenStream> {
        self.paths.iter().find(|(c, _)| c == class).map(|(_, p)| p)
    }

    /// The wrapper type, its constructor, and the trait impls it needs.
    ///
    /// The constructor is the only place virtual-base links are written: it places each shared
    /// base once, then walks every subobject that names one and records the distance to it.
    fn expand_complete(
        &self,
        class: &Ident,
        vis: &Visibility,
        ancestors: &[&Ancestor],
    ) -> syn::Result<TokenStream> {
        if !self.wrapped {
            return Ok(TokenStream::new());
        }
        let complete = &self.complete;
        let sub_field = base_field(class);
        let vfields: Vec<Ident> = self.vbases.iter().map(base_field).collect();
        let vbases = &self.vbases;

        let requested = inherited_dyn_traits(ancestors);
        let derives: Vec<TokenStream> = requested
            .iter()
            .filter_map(|t| match t.to_string().as_str() {
                "Debug" => Some(quote!(::core::fmt::Debug)),
                "PartialEq" => Some(quote!(::core::cmp::PartialEq)),
                "Eq" => Some(quote!(::core::cmp::Eq)),
                "Hash" => Some(quote!(::core::hash::Hash)),
                // `Clone` is written out below rather than derived -- see there.
                _ => None,
            })
            .collect();
        // Deriving on the wrapper is what makes the shared base take part exactly once: it is a
        // field here and nowhere else, so it is compared and hashed with the object rather than
        // once per path that reaches it.
        let derive_attr = (!derives.is_empty()).then(|| quote!(#[derive(#(#derives),*)]));

        // Cloning cannot be derived. Every virtual-base link is an offset that only means anything
        // inside the complete object it was built for, and cloning the class subobject clears its
        // links precisely so a stray copy cannot resolve one. Rebuilding through the constructor
        // is what makes the copy whole again.
        let clone = requested.iter().any(|t| t == "Clone").then(|| {
            let vf = &vfields;
            quote! {
                impl ::core::clone::Clone for #complete {
                    #[inline]
                    fn clone(&self) -> Self {
                        #class::complete(
                            ::core::clone::Clone::clone(&self.#sub_field),
                            #(::core::clone::Clone::clone(&self.#vf),)*
                        )
                    }
                }
            }
        });

        let display = requested.iter().any(|t| t == "Display").then(|| {
            quote! {
                impl ::core::fmt::Display for #complete {
                    #[inline]
                    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        ::core::fmt::Display::fmt(&self.#sub_field, f)
                    }
                }
            }
        });

        let mut fixups = Vec::new();
        for a in ancestors {
            let Some(owner) = self.path(&a.class) else {
                continue;
            };
            for v in a.virtual_bases() {
                let Some(shared) = self.path(&v.class) else {
                    continue;
                };
                let slot = base_field(&v.class);
                fixups.push(quote! {
                    // SAFETY: both operands are `offset_of!` constants of the same `#[repr(C)]`
                    // complete type, so their difference is exactly the distance from this
                    // subobject to the single shared base -- which is what `link` requires.
                    obj.#owner.#slot = unsafe {
                        ::obj::__private::VBase::link(
                            ::obj::__private::offset_of!(#complete, #shared) as isize
                                - ::obj::__private::offset_of!(#complete, #owner) as isize,
                        )
                    };
                });
            }
        }

        let shared_list = vbases
            .iter()
            .map(|v| format!("`{v}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let struct_doc = format!(
            "A complete `{class}` object, including the shared base subobjects.\n\nBecause \
             `{class}` inherits {shared_list} virtually, the shared copy cannot live inside \
             `{class}` itself — every path through the hierarchy has to reach the *same* one. It \
             lives here instead, which is why [`Obj::<{class}>::new`](obj::Obj::new) takes this \
             type rather than `{class}`.\n\nBuild one with [`{class}::complete`].",
        );
        let ctor_doc = format!(
            "Assembles a complete `{class}`, placing {shared_list} once and linking every \
             subobject that shares it.\n\nThis is the `obj` equivalent of a C++ most-derived \
             constructor, which is likewise the only one that initialises virtual bases.\n\n\
             ```ignore\nlet obj = Obj::<{class}>::new({class}::complete(sub, shared));\n```",
        );

        Ok(quote! {
            #[doc = #struct_doc]
            #[repr(C)]
            #derive_attr
            #vis struct #complete {
                #sub_field: #class,
                #(#vfields: #vbases,)*
            }

            // The class subobject leads, so the complete object and the class share an address
            // and every table entry measured from the wrapper is also correct from the class.
            const _: () = assert!(
                ::obj::__private::offset_of!(#complete, #sub_field) == 0,
                "obj: the class subobject must lead its complete object",
            );

            impl #class {
                #[doc = #ctor_doc]
                #[must_use]
                #vis fn complete(#sub_field: #class, #(#vfields: #vbases),*) -> #complete {
                    let mut obj = #complete { #sub_field, #(#vfields),* };
                    #(#fixups)*
                    obj
                }
            }

            #clone
            #display
        })
    }
}

pub enum Kind {
    Class {
        bases: Vec<BaseRef>,
    },
    Methods {
        virtual_sigs: TokenStream,
        provides: Vec<Ident>,
    },
}

/// Where the delegate subobject lives inside the target.
pub enum Route {
    /// The target implements its own interface, so the receiver is `self`.
    Own,
    /// A base stored inline, reached by field access.
    Field(Ident),
    /// A shared base, reached by resolving this class's `VBase` link.
    Virtual,
}

/// How a class reaches the implementation of a method it does not provide itself.
pub struct Delegate {
    /// The base to hand the call to.
    pub class: Ident,
    /// How to get from the target to that base's subobject.
    pub route: Route,
    /// Whether that base is abstract, and so implements no interface to name.
    pub is_abstract: bool,
}

impl Parse for Delegate {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let class = content.parse()?;
        if content.peek(Token![self]) {
            content.parse::<Token![self]>()?;
            return Ok(Delegate {
                class,
                route: Route::Own,
                is_abstract: false,
            });
        }
        let route = if content.peek(Token![virtual]) {
            content.parse::<Token![virtual]>()?;
            Route::Virtual
        } else {
            Route::Field(content.parse()?)
        };
        let is_abstract = content.parse::<LitBool>()?.value;
        Ok(Delegate {
            class,
            route,
            is_abstract,
        })
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
            let (c, ab, v, b, d) = (
                &a.class,
                a.is_abstract,
                &a.vis,
                &a.direct_bases,
                &a.dyn_traits,
            );
            quote!((#c #ab #v [#(#b)*] [#(#d)*]))
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
            match &delegate.route {
                // Implementing its own interface: the inherent method must exist, and its
                // absence is exactly the "pure virtual never overridden" error.
                Route::Own => quote!(#target::#name(self #(, #args)*)),
                route => {
                    let is_mut = sig.receiver().and_then(|r| r.mutability).is_some();
                    let mutability = is_mut.then(|| quote!(mut));
                    let base = &delegate.class;
                    // How to name the delegate's subobject. A shared base is not a field of this
                    // class, only a link, so it is reached through the generated accessor.
                    let recv = match route {
                        Route::Field(field) => quote!(& #mutability self.#field),
                        Route::Virtual => {
                            let acc = if is_mut {
                                base_accessor_mut(base)
                            } else {
                                base_accessor(base)
                            };
                            quote!(self.#acc())
                        }
                        Route::Own => unreachable!("handled above"),
                    };
                    if base == owner {
                        // The provider is the interface's own class: call its inherent body.
                        quote!(#owner::#name(#recv #(, #args)*))
                    } else if delegate.is_abstract {
                        // An *abstract* intermediate implements no interface, so there is no
                        // `<Base as OwnerDyn>` to name. Method-call syntax resolves it instead:
                        // starting at the base subobject it finds that base's own inherent
                        // override first, then its interface impl if it has one, and otherwise
                        // derefs on up the chain to whichever ancestor supplied the body. It can
                        // never pick this impl back up, because the receiver is a strictly
                        // shallower subobject than `self`.
                        quote!((#recv).#name(#(#args),*))
                    } else {
                        // A concrete intermediate: name its interface impl exactly, which
                        // resolves the same question one level up.
                        quote!(<#base as #owner_iface>::#name(#recv #(, #args)*))
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

fn self_entry(class: &Ident, complete: &Ident, is_abstract: bool) -> TokenStream {
    let iface = iface_trait(class);
    let vtable = if is_abstract {
        // An abstract class implements no interface, not even its own, so there is no vtable to
        // record. Its table is never the one a cast consults, because it can never be the
        // most-derived class of a live object.
        quote!(::core::option::Option::None)
    } else {
        // The vtable belongs to the *complete* object, which is the type that implements the
        // interface and the type a live handle actually points at.
        quote!(::core::option::Option::Some(
            ::obj::__vtable_of!(#complete as dyn #iface)
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
    bases: &[BaseRef],
    ancestors: &[&Ancestor],
) -> syn::Result<TokenStream> {
    let me = ancestors
        .first()
        .ok_or_else(|| syn::Error::new(class.span(), "obj: empty ancestor list"))?;
    let vis = &me.vis;
    let is_abstract = me.is_abstract;
    let layout = Layout::of(class, is_abstract, ancestors)?;
    let complete = &layout.complete;

    let iface = iface_trait(class);
    let sub_tr = sub_trait(class);
    let bases_tr = bases_trait(class);
    let table = table_static(class);
    let meta = meta_static(class);
    let sub_fn = sub_fn(class);
    let sub_fn_mut = sub_fn_mut(class);
    let name_lit = LitStr::new(&class.to_string(), class.span());
    let entry = self_entry(class, complete, is_abstract);
    let shares_bases = layout.wrapped;

    // Where each subobject sits inside the complete object. Without virtual bases this is just
    // the field chain from the class; with them, everything is measured from the wrapper instead,
    // because that is the type a live object actually has.
    let offset_of = |c: &Ident| -> syn::Result<TokenStream> {
        let path = layout.path(c).ok_or_else(|| {
            syn::Error::new(
                class.span(),
                format!("obj: no path from `{class}` to its `{c}` subobject"),
            )
        })?;
        Ok(quote!(::obj::__private::offset_of!(#complete, #path)))
    };

    // One sub-table per base, each shifted by where that base sits. Concrete classes go through
    // the base's `__ObjBases*` trait, which rewrites every inherited entry to carry *this* class's
    // vtables.
    let branch = |b: &Ident| -> syn::Result<TokenStream> {
        let at = offset_of(b)?;
        let b_bases = bases_trait(b);
        Ok(if is_abstract {
            quote! {
                ::obj::BaseTable::from_slice_without_vtables(<#b as ::obj::Class>::META.bases)
                    .offset_by(#at)
            }
        } else {
            quote!(<#complete as #b_bases>::OBJ_TABLE.offset_by(#at))
        })
    };

    // Entries for what lies inside the class's own layout: its non-virtual bases, then itself.
    let stored: Vec<&Ident> = bases
        .iter()
        .filter(|b| !b.is_virtual)
        .map(|b| &b.class)
        .collect();
    let inline_table = match stored.split_first() {
        Some((primary, secondaries)) => {
            let head = branch(primary)?;
            let mut expr = quote!(#head);
            for b in secondaries {
                let t = branch(b)?;
                expr = quote!(#expr.concat(#t));
            }
            quote!(#expr.push(#entry))
        }
        None => quote!(::obj::BaseTable::EMPTY.push(#entry)),
    };

    // Virtual bases are appended by whoever *stores* them -- never carried up through the
    // `__ObjBases` chain, because their offset is not fixed relative to any intermediate class.
    // That is also what deduplicates a diamond: however many paths reach the shared base, only
    // the complete object contributes its entry, once.
    let mut table_expr = inline_table.clone();
    for v in &layout.vbases {
        let at = offset_of(v)?;
        let v_bases = bases_trait(v);
        table_expr = quote!(#table_expr.concat(<#complete as #v_bases>::OBJ_TABLE.offset_by(#at)));
    }

    // The same construction for an arbitrary subclass `C`, so descendants inherit entries for
    // ancestors they were never told about. Offsets here are relative to *this* class, since the
    // caller shifts the whole table by wherever it put this subobject.
    let bases_impl = {
        let extra_bounds = stored.iter().map(|b| {
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
        let expr = match stored.split_first() {
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
        // `C` is whatever type implements the interface -- the class itself, or a subclass's
        // `Complete` wrapper, which is not a `Class`. So the bound is the interface, not `Class`.
        quote! {
            impl<C> #bases_tr for C
            where
                C: #iface + Sized #(#extra_bounds)*,
            {
                const OBJ_TABLE: ::obj::BaseTable = #expr;
            }
        }
    };

    // A class with virtual bases needs a second, smaller description of itself, for a *bare*
    // subobject that is not part of a complete object. Such a subobject genuinely cannot say
    // where the shared bases are, so its table omits them -- and carries no vtables either, since
    // it can never be a live object. Without this, resolving a shared base from a bare subobject
    // would read past the end of it.
    let sub_table = sub_table_static(class);
    let sub_meta = sub_meta_static(class);
    let (self_meta, sub_meta_def) = if layout.wrapped {
        (
            quote!(#sub_meta),
            quote! {
                #[doc(hidden)]
                #vis static #sub_table: ::obj::BaseTable = #inline_table.without_vtables();

                #[doc(hidden)]
                // A bare subobject stores no shared base, so nothing may be resolved from it.
                #vis static #sub_meta: ::obj::ClassMeta = ::obj::ClassMeta {
                    name: #name_lit,
                    id: ::obj::__private::TypeId::of::<#class>,
                    bases: #sub_table.as_slice(),
                    shares_bases: false,
                };
            },
        )
    } else {
        (quote!(#meta), quote!())
    };

    let complete_def = layout.expand_complete(class, vis, ancestors)?;

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
                #[inline]
                fn up_rc(
                    this: ::obj::__private::Rc<dyn #iface>,
                ) -> ::obj::__private::Rc<dyn #anc_iface> { this }
                #[inline]
                fn up_arc(
                    this: ::obj::__private::Arc<dyn #iface + Send + Sync>,
                ) -> ::obj::__private::Arc<dyn #anc_iface + Send + Sync> { this }
            }
        }
    });

    // The shim impls behind `dyn_traits(..)`. Abstract classes implement no interface, so nothing
    // requires these of them -- and demanding `Clone` or `PartialEq` of a class that can never be
    // instantiated would be a pointless bound on the user.
    // These are required of every type that implements the interface. That is the complete object,
    // and — when the two differ — the class itself as well, since the class type is what a
    // subclass delegates to.
    let dyn_trait_impls: Vec<TokenStream> = if is_abstract {
        Vec::new()
    } else {
        let requested = inherited_dyn_traits(ancestors);
        let complete_ty = quote!(#complete);
        let mut out: Vec<TokenStream> = requested
            .iter()
            .filter_map(|t| dyn_trait_impl(t, &complete_ty))
            .collect();
        if layout.wrapped {
            let class_ty = quote!(#class);
            out.extend(
                requested
                    .iter()
                    .filter_map(|t| dyn_trait_impl(t, &class_ty)),
            );
        }
        out
    };

    let concrete = (!is_abstract).then(|| {
        quote! {
            unsafe impl ::obj::Concrete for #class {
                #[inline]
                fn into_dyn(
                    value: ::obj::__private::Box<#complete>,
                ) -> ::obj::__private::Box<dyn #iface> { value }
                #[inline]
                fn as_dyn(value: &#complete) -> &(dyn #iface + 'static) { value }
                #[inline]
                fn as_dyn_mut(value: &mut #complete) -> &mut (dyn #iface + 'static) { value }
                #[inline]
                fn rc_into_dyn(
                    value: ::obj::__private::Rc<#complete>,
                ) -> ::obj::__private::Rc<dyn #iface> { value }
                #[inline]
                fn arc_into_dyn(
                    value: ::obj::__private::Arc<#complete>,
                ) -> ::obj::__private::Arc<dyn #iface + Send + Sync>
                where
                    #complete: Send + Sync,
                { value }
            }
        }
    });

    // The wrapper is the live object, so it is what answers "what class am I, and where does the
    // complete object start". The bare class keeps its own, vtable-free answer.
    let complete_any_obj = layout.wrapped.then(|| {
        quote! {
            unsafe impl ::obj::AnyObj for #complete {
                #[inline]
                fn class_meta(&self) -> &'static ::obj::ClassMeta { &#meta }
                #[inline]
                fn obj_addr(&self) -> *const u8 { (self as *const Self).cast::<u8>() }
            }
        }
    });

    Ok(quote! {
        // Locates this class's subobject inside any object that derives from it. Resolving the
        // offset at runtime is what frees a class from knowing the layout of its descendants.
        #[doc(hidden)]
        #[allow(missing_docs)]
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

        // `dyn X + Send + Sync` is a distinct type from `dyn X`, so the thread-safe interface
        // that `ArcShared` stores needs its own field access.
        impl ::obj::__private::Deref for dyn #iface + Send + Sync {
            type Target = #class;
            #[inline]
            fn deref(&self) -> &#class { self.#sub_fn() }
        }
        impl ::obj::__private::DerefMut for dyn #iface + Send + Sync {
            #[inline]
            fn deref_mut(&mut self) -> &mut #class { self.#sub_fn_mut() }
        }

        unsafe impl ::obj::AnyObj for #class {
            #[inline]
            fn class_meta(&self) -> &'static ::obj::ClassMeta { &#self_meta }
            #[inline]
            fn obj_addr(&self) -> *const u8 { (self as *const Self).cast::<u8>() }
        }

        #complete_def
        #complete_any_obj

        #[doc(hidden)]
        #vis static #table: ::obj::BaseTable = #table_expr;

        #[doc(hidden)]
        #vis static #meta: ::obj::ClassMeta = ::obj::ClassMeta {
            name: #name_lit,
            id: ::obj::__private::TypeId::of::<#class>,
            bases: #table.as_slice(),
            shares_bases: #shares_bases,
        };

        #sub_meta_def

        unsafe impl ::obj::Class for #class {
            type Dyn = dyn #iface;
            type SendDyn = dyn #iface + Send + Sync;
            type Complete = #complete;
            const META: &'static ::obj::ClassMeta = &#meta;
            #[inline]
            fn send_as_dyn<'a>(
                value: &'a (dyn #iface + Send + Sync + 'static),
            ) -> &'a (dyn #iface + 'static) { value }
        }

        #[doc(hidden)]
        #[allow(missing_docs)]
        #vis trait #bases_tr: #iface + Sized {
            const OBJ_TABLE: ::obj::BaseTable;
        }
        #bases_impl

        #(#upcasts)*
        #concrete
        #(#dyn_trait_impls)*
    })
}

/// Walks the class graph to find whether `from` is, or derives from, `target`.
fn reaches(graph: &[(&Ident, &Vec<BaseRef>)], from: &Ident, target: &Ident) -> bool {
    if from == target {
        return true;
    }
    graph
        .iter()
        .find(|(name, _)| *name == from)
        .is_some_and(|(_, bases)| bases.iter().any(|b| reaches(graph, &b.class, target)))
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
    let mut supertraits = if me.direct_bases.is_empty() {
        quote!(::obj::AnyObj)
    } else {
        let each = me.direct_bases.iter().map(|b| iface_trait(&b.class));
        quote!(#(#each)+*)
    };

    // Only this class's *own* `dyn_traits(..)` are added here: a subclass inherits them through
    // its base's interface, which is already a supertrait.
    for t in &me.dyn_traits {
        let path = dyn_trait_path(t).ok_or_else(|| {
            syn::Error::new(t.span(), format!("obj: unknown `dyn_traits` entry `{t}`"))
        })?;
        supertraits = quote!(#supertraits + #path);
    }

    let graph: Vec<(&Ident, &Vec<BaseRef>)> = ancestors
        .iter()
        .map(|a| (&a.class, &a.direct_bases))
        .collect();
    let is_abstract = |c: &Ident| ancestors.iter().any(|a| a.class == *c && a.is_abstract);

    let layout = Layout::of(class, me.is_abstract, ancestors)?;
    let complete = &layout.complete;

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
                let Some(via) = me
                    .direct_bases
                    .iter()
                    .find(|d| reaches(&graph, &d.class, owner))
                else {
                    return Err(syn::Error::new(
                        class.span(),
                        format!("obj: no base of `{class}` reaches `{owner}`"),
                    ));
                };
                let base = &via.class;
                let abstract_base = is_abstract(base);
                if via.is_virtual {
                    quote!((#base virtual #abstract_base))
                } else {
                    let field = base_field(base);
                    quote!((#base #field #abstract_base))
                }
            };
            calls.push(quote!(#mac! { #class, #delegate, [#(#provides)*] }));

            // The complete object is what a live handle points at, so it is what has to implement
            // every interface. It owns no behaviour of its own -- it forwards each method straight
            // to the class subobject it wraps, which resolved the dispatch already.
            if layout.wrapped {
                let sub_field = base_field(class);
                calls.push(quote!(#mac! { #complete, (#class #sub_field false), [] }));
            }
        }
        Some(quote!(#(#calls)*))
    };

    let doc = format!(
        "The polymorphic interface of [`{class}`].\n\nGenerated by `#[obj::methods]`. Carries \
         `{class}`'s virtual methods, and has each base class's interface as a supertrait so that \
         `dyn` upcasting works. You rarely name this directly: [`Obj`](obj::Obj), \
         [`Ref`](obj::Ref) and friends store it for you.",
    );

    Ok(quote! {
        #[doc = #doc]
        #[allow(missing_docs)]
        #vis trait #iface: #supertraits + #sub_tr {
            #virtual_sigs
        }

        #iface_impls
    })
}
