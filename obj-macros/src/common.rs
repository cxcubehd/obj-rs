//! Naming conventions shared by the macros.
//!
//! A class `X` is compiled into a family of items whose names are derived mechanically from `X`.
//! That is what lets a class refer to its base's generated items while knowing nothing but the
//! base's *name*.

use proc_macro2::Span;
use syn::Ident;

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

/// The field holding the base subobject, named after the base class in snake_case.
///
/// Always the first field, so prefix layout holds. Naming it after the class keeps constructors
/// readable: `Square { polygon: Polygon { shape: Shape { x }, sides }, s }`.
pub fn base_field(base: &Ident) -> Ident {
    Ident::new(&to_snake_case(&base.to_string()), Span::call_site())
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
