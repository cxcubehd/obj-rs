//! `#[obj::class]` — the data half of a class declaration.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Field, Fields, Ident, ItemStruct, Token};

use crate::common::*;

/// Arguments to `#[obj::class(...)]`.
pub struct ClassArgs {
    /// The base class, if any.
    pub extends: Option<Ident>,
    /// Whether this class has pure virtual methods and so cannot be instantiated.
    pub is_abstract: bool,
}

impl Parse for ClassArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut extends = None;
        let mut is_abstract = false;
        while !input.is_empty() {
            let look = input.lookahead1();
            if look.peek(Token![abstract]) {
                input.parse::<Token![abstract]>()?;
                is_abstract = true;
            } else if look.peek(Ident) {
                let key: Ident = input.parse()?;
                if key == "extends" {
                    input.parse::<Token![=]>()?;
                    extends = Some(input.parse()?);
                } else {
                    return Err(syn::Error::new(
                        key.span(),
                        "obj: expected `extends = Base` or `abstract`",
                    ));
                }
            } else {
                return Err(look.error());
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(ClassArgs {
            extends,
            is_abstract,
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

    // Prefix layout: the base subobject goes first, so `&Derived` is also a valid `&Base`.
    if let Some(base) = &args.extends {
        let base_field = base_field(base);
        let field: Field = syn::parse_quote!(#vis #base_field: #base);
        match &mut item.fields {
            Fields::Named(named) => named.named.insert(0, field),
            other => {
                let mut named: syn::FieldsNamed = syn::parse_quote!({});
                named.named.push(field);
                *other = Fields::Named(named);
            }
        }
    }
    item.attrs.insert(0, syn::parse_quote!(#[repr(C)]));

    let ancestors_mac = ancestors_macro(&class);
    let is_abstract = args.is_abstract;

    // Each class contributes its own name to a list built by walking up the chain. A root
    // terminates the walk by handing the whole list to the emitter.
    let ancestors_def = match &args.extends {
        Some(base) => {
            let base_mac = ancestors_macro(base);
            quote! {
                #[doc(hidden)]
                #[macro_export]
                macro_rules! #ancestors_mac {
                    ({$($pre:tt)*} [$($acc:tt)*]) => {
                        #base_mac! { {$($pre)*} [$($acc)* (#class #is_abstract #vis)] }
                    };
                }
            }
        }
        None => quote! {
            #[doc(hidden)]
            #[macro_export]
            macro_rules! #ancestors_mac {
                ({$($pre:tt)*} [$($acc:tt)*]) => {
                    ::obj::__obj_emit! {
                        $($pre)* ancestors [$($acc)* (#class #is_abstract #vis)]
                    }
                };
            }
        },
    };

    let deref = args.extends.as_ref().map(|base| {
        let base_field = base_field(base);
        quote! {
            // Inherits the base's fields and non-virtual methods.
            impl ::obj::__private::Deref for #class {
                type Target = #base;
                #[inline]
                fn deref(&self) -> &#base { &self.#base_field }
            }
            impl ::obj::__private::DerefMut for #class {
                #[inline]
                fn deref_mut(&mut self) -> &mut #base { &mut self.#base_field }
            }
            const _: () = assert!(
                ::obj::__private::offset_of!(#class, #base_field) == 0,
                "obj: the base subobject must be at offset 0",
            );
        }
    });

    let base_tok = match &args.extends {
        Some(b) => quote!(base #b),
        None => quote!(root),
    };

    Ok(quote! {
        #item
        #deref
        #ancestors_def

        // Walk the chain, then emit everything that needs the full ancestor list.
        #ancestors_mac! { {class #class #base_tok} [] }
    })
}
