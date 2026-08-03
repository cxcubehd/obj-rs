//! The tail of both macros: everything that needs the *full* ancestor list.
//!
//! Neither attribute macro can see past its own item, so a class knows only its direct base. The
//! `__obj_ancestors_*` chain of `macro_rules` walks up the hierarchy accumulating names and hands
//! the complete list here.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitBool, LitStr, Visibility};

use crate::common::*;

/// One entry of the accumulated ancestor list: `(Class is_abstract vis)`.
pub struct Ancestor {
    pub class: Ident,
    pub is_abstract: bool,
    pub vis: Visibility,
}

impl Parse for Ancestor {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let content;
        syn::parenthesized!(content in input);
        let class = content.parse()?;
        let is_abstract = content.parse::<LitBool>()?.value;
        let vis = content.parse()?;
        Ok(Ancestor {
            class,
            is_abstract,
            vis,
        })
    }
}

pub enum EmitInput {
    Class {
        class: Ident,
        base: Option<Ident>,
        ancestors: Vec<Ancestor>,
    },
    Methods {
        class: Ident,
        virtual_sigs: TokenStream,
        ancestors: Vec<Ancestor>,
    },
}

impl Parse for EmitInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let kind: Ident = input.parse()?;
        let class: Ident = input.parse()?;

        let result = if kind == "class" {
            let marker: Ident = input.parse()?;
            let base = if marker == "base" {
                Some(input.parse()?)
            } else {
                None
            };
            EmitInput::Class {
                class,
                base,
                ancestors: Vec::new(),
            }
        } else if kind == "methods" {
            let content;
            syn::braced!(content in input);
            EmitInput::Methods {
                class,
                virtual_sigs: content.parse()?,
                ancestors: Vec::new(),
            }
        } else {
            return Err(syn::Error::new(kind.span(), "obj: internal macro misuse"));
        };

        let marker: Ident = input.parse()?;
        if marker != "ancestors" {
            return Err(syn::Error::new(marker.span(), "obj: internal macro misuse"));
        }
        let list;
        syn::bracketed!(list in input);
        let mut ancestors = Vec::new();
        while !list.is_empty() {
            ancestors.push(list.parse()?);
        }

        Ok(match result {
            EmitInput::Class { class, base, .. } => EmitInput::Class {
                class,
                base,
                ancestors,
            },
            EmitInput::Methods {
                class,
                virtual_sigs,
                ..
            } => EmitInput::Methods {
                class,
                virtual_sigs,
                ancestors,
            },
        })
    }
}

pub fn expand(input: EmitInput) -> syn::Result<TokenStream> {
    match input {
        EmitInput::Class {
            class,
            base,
            ancestors,
        } => expand_class(&class, base.as_ref(), &ancestors),
        EmitInput::Methods {
            class,
            virtual_sigs,
            ancestors,
        } => expand_methods(&class, &virtual_sigs, &ancestors),
    }
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
    base: Option<&Ident>,
    ancestors: &[Ancestor],
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

    // The class's own table. Concrete classes chain through the base's `__ObjBases*` trait, which
    // rewrites every inherited entry to carry *this* class's vtables.
    let table_expr = match (base, is_abstract) {
        (Some(b), false) => {
            let b_bases = bases_trait(b);
            let base_field = base_field(b);
            quote! {
                <#class as #b_bases>::OBJ_TABLE
                    .offset_by(::obj::__private::offset_of!(#class, #base_field))
                    .push(#entry)
            }
        }
        (Some(b), true) => {
            let base_field = base_field(b);
            quote! {
                ::obj::BaseTable::from_slice_without_vtables(
                    <#b as ::obj::Class>::META.bases,
                )
                .offset_by(::obj::__private::offset_of!(#class, #base_field))
                .push(#entry)
            }
        }
        (None, _) => quote!(::obj::BaseTable::EMPTY.push(#entry)),
    };

    // Hands any subclass the entries for this class and everything above it.
    let bases_impl = match base {
        Some(b) => {
            let b_bases = bases_trait(b);
            let base_field = base_field(b);
            quote! {
                impl<C> #bases_tr for C
                where
                    C: ::obj::Class + #iface + Sized + ::obj::SubclassOf<#class> + #b_bases,
                {
                    const OBJ_TABLE: ::obj::BaseTable = <C as #b_bases>::OBJ_TABLE
                        .offset_by(::obj::__private::offset_of!(#class, #base_field))
                        .push(::obj::BaseEntry {
                            id: ::obj::__private::TypeId::of::<#class>,
                            data_offset: 0,
                            dyn_vtable: ::core::option::Option::Some(
                                ::obj::__vtable_of!(C as dyn #iface),
                            ),
                        });
                }
            }
        }
        None => quote! {
            impl<C> #bases_tr for C
            where
                C: ::obj::Class + #iface + Sized + ::obj::SubclassOf<#class>,
            {
                const OBJ_TABLE: ::obj::BaseTable = ::obj::BaseTable::EMPTY.push(
                    ::obj::BaseEntry {
                        id: ::obj::__private::TypeId::of::<#class>,
                        data_offset: 0,
                        dyn_vtable: ::core::option::Option::Some(
                            ::obj::__vtable_of!(C as dyn #iface),
                        ),
                    },
                );
            }
        },
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
        // offset at runtime is what frees a class from knowing the layout of descendants.
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

fn expand_methods(
    class: &Ident,
    virtual_sigs: &TokenStream,
    ancestors: &[Ancestor],
) -> syn::Result<TokenStream> {
    let me = ancestors
        .first()
        .ok_or_else(|| syn::Error::new(class.span(), "obj: empty ancestor list"))?;
    let vis = &me.vis;
    let iface = iface_trait(class);
    let sub_tr = sub_trait(class);

    // The interface's supertrait chain mirrors the class chain, so `dyn Derived` upcasts to
    // `dyn Base` for free.
    let supertrait = match ancestors.get(1) {
        Some(parent) => {
            let p = iface_trait(&parent.class);
            quote!(#p)
        }
        None => quote!(::obj::AnyObj),
    };

    // Implement every ancestor's interface for this class. Each ancestor generated a
    // `__obj_iface_*` macro that knows its own virtual methods.
    let iface_impls = (!me.is_abstract).then(|| {
        let calls = ancestors.iter().map(|a| {
            let mac = iface_macro(&a.class);
            quote!(#mac! { #class })
        });
        quote!(#(#calls)*)
    });

    Ok(quote! {
        #vis trait #iface: #supertrait + #sub_tr {
            #virtual_sigs
        }

        #iface_impls
    })
}
