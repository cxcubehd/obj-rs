//! Naming conventions shared by the macros.
//!
//! A class `X` is compiled into a family of items whose names are derived mechanically from `X`.
//! That is what lets a class refer to its base's generated items while knowing nothing but the
//! base's *name*.

use proc_macro2::{Span, TokenStream};
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream};
use syn::{Ident, Token};

/// A base class as written in `extends(..)`: either `Base` or `virtual Base`.
///
/// A virtual base is shared: however many paths reach it, the complete object holds one copy, and
/// each subobject that names it stores a [`VBase`](../obj/vbase/struct.VBase.html) link instead of
/// the base itself.
#[derive(Clone)]
pub struct BaseRef {
    pub class: Ident,
    pub is_virtual: bool,
}

impl Parse for BaseRef {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let is_virtual = input.peek(Token![virtual]);
        if is_virtual {
            input.parse::<Token![virtual]>()?;
        }
        Ok(BaseRef {
            class: input.parse()?,
            is_virtual,
        })
    }
}

impl ToTokens for BaseRef {
    fn to_tokens(&self, out: &mut TokenStream) {
        if self.is_virtual {
            out.extend(quote!(virtual));
        }
        self.class.to_tokens(out);
    }
}

/// `X` -> `XDyn`, the class's `dyn`-safe interface carrying its virtual methods.
pub fn iface_trait(class: &Ident) -> Ident {
    Ident::new(&format!("{class}Dyn"), class.span())
}

/// `X` -> `__obj_sub_x`, the accessor that locates an `X` subobject inside any object.
pub fn sub_fn(class: &Ident) -> Ident {
    Ident::new(
        &format!("__obj_sub_{}", to_snake_case(&class.to_string())),
        Span::call_site(),
    )
}

/// `X` -> `__obj_sub_x_mut`.
pub fn sub_fn_mut(class: &Ident) -> Ident {
    Ident::new(
        &format!("__obj_sub_{}_mut", to_snake_case(&class.to_string())),
        Span::call_site(),
    )
}

/// `X` -> `__ObjSubX`, the blanket trait that locates an `X` subobject inside any object.
pub fn sub_trait(class: &Ident) -> Ident {
    Ident::new(&format!("__ObjSub{class}"), Span::call_site())
}

/// `X` -> `__ObjBasesX`, the trait that hands a subclass the base-table entries for `X` and
/// everything `X` inherits from.
pub fn bases_trait(class: &Ident) -> Ident {
    Ident::new(&format!("__ObjBases{class}"), Span::call_site())
}

/// `X` -> `__obj_ancestors_X`, the recursive `macro_rules` that walks up the chain collecting
/// ancestor names.
pub fn ancestors_macro(class: &Ident) -> Ident {
    Ident::new(&format!("__obj_ancestors_{class}"), Span::call_site())
}

/// `X` -> `__obj_iface_X`, the `macro_rules` that implements `XDyn` for a given subclass.
pub fn iface_macro(class: &Ident) -> Ident {
    Ident::new(&format!("__obj_iface_{class}"), Span::call_site())
}

/// `X` -> `__OBJ_TABLE_X`, the class's base table.
pub fn table_static(class: &Ident) -> Ident {
    Ident::new(&format!("__OBJ_TABLE_{class}"), Span::call_site())
}

/// `X` -> `__OBJ_META_X`, the class's runtime metadata.
pub fn meta_static(class: &Ident) -> Ident {
    Ident::new(&format!("__OBJ_META_{class}"), Span::call_site())
}

/// `X` -> `XComplete`, the type that owns a complete object of a class with virtual bases.
///
/// The shared bases cannot live inside `X` itself — the most-derived class decides where the one
/// copy goes — so they are stored alongside it here. [`Class::Complete`] names this type, and it
/// is what actually implements the class's interface.
///
/// [`Class::Complete`]: ../obj/class/trait.Class.html#associatedtype.Complete
pub fn complete_type(class: &Ident) -> Ident {
    Ident::new(&format!("{class}Complete"), class.span())
}

/// `X` -> `__OBJ_SUB_TABLE_X`, the base table describing a bare `X` subobject.
pub fn sub_table_static(class: &Ident) -> Ident {
    Ident::new(&format!("__OBJ_SUB_TABLE_{class}"), Span::call_site())
}

/// `X` -> `__OBJ_SUB_META_X`, the metadata of a bare `X` subobject.
///
/// A class with virtual bases needs two descriptions of itself. The complete object knows where
/// the shared bases are; a bare `X` subobject does not, because it is not the thing that stores
/// them — so its table lists only what really lies inside it, and carries no vtables.
pub fn sub_meta_static(class: &Ident) -> Ident {
    Ident::new(&format!("__OBJ_SUB_META_{class}"), Span::call_site())
}

/// `X` -> `as_x`, the accessor for a secondary or virtual base subobject.
pub fn base_accessor(base: &Ident) -> Ident {
    Ident::new(
        &format!("as_{}", to_snake_case(&base.to_string())),
        base.span(),
    )
}

/// `X` -> `as_x_mut`.
pub fn base_accessor_mut(base: &Ident) -> Ident {
    Ident::new(
        &format!("as_{}_mut", to_snake_case(&base.to_string())),
        base.span(),
    )
}

/// The field holding the base subobject, named after the base class in snake_case.
///
/// Always the first field, so prefix layout holds. Naming it after the class keeps constructors
/// readable: `Square { polygon: Polygon { shape: Shape { x }, sides }, s }`.
pub fn base_field(base: &Ident) -> Ident {
    Ident::new(&to_snake_case(&base.to_string()), Span::call_site())
}

/// The standard traits a class may opt into with `#[obj::class(dyn_traits(..))]`.
///
/// Listed in the order they are emitted, so generated code is deterministic regardless of the
/// order the user wrote them in.
pub const DYN_TRAITS: &[&str] = &["Debug", "Display", "Clone", "PartialEq", "Eq", "Hash"];

/// Puts `dyn_traits` entries into [`DYN_TRAITS`] order.
pub fn sort_dyn_traits(traits: &mut [Ident]) {
    traits.sort_by_key(|t| {
        DYN_TRAITS
            .iter()
            .position(|known| t == *known)
            .unwrap_or(usize::MAX)
    });
}

/// `HttpRequest` -> `http_request`
pub fn to_snake_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.char_indices() {
        if ch.is_uppercase() {
            if i != 0 {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}
