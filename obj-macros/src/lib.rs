//! Procedural macros for the [`obj`](https://docs.rs/obj) crate.
//!
//! These are re-exported by `obj`; depend on that crate rather than this one.

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod class;
mod common;
mod emit;
mod methods;

/// Declares the data half of a class.
///
/// ```ignore
/// #[obj::class]                       // a root class
/// pub struct Shape { pub x: f64 }
///
/// #[obj::class(extends = Shape)]      // a derived class
/// pub struct Circle { pub r: f64 }
///
/// #[obj::class(abstract)]             // has pure virtual methods; cannot be instantiated
/// pub struct Drawable { }
/// ```
///
/// The struct is given `#[repr(C)]` and, when it has a base, the base subobject is inserted as its
/// first field. Every class must also have an `#[obj::methods]` block, which declares its virtual
/// methods; write an empty one if it has none.
#[proc_macro_attribute]
pub fn class(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as class::ClassArgs);
    let item = parse_macro_input!(item as syn::ItemStruct);
    class::expand(args, item)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Declares the behaviour half of a class.
///
/// ```ignore
/// #[obj::methods]
/// impl Shape {
///     #[obj(virtual)] fn area(&self) -> f64;              // pure virtual
///     #[obj(virtual)] fn scale(&mut self, k: f64) { .. }  // virtual with a default body
///     fn describe(&self) -> String { .. }                 // non-virtual, inherited via Deref
/// }
///
/// #[obj::methods]
/// impl Circle {
///     #[obj(override)] fn area(&self) -> f64 { .. }
/// }
/// ```
///
/// Every method also becomes an inherent method, which is what a `super` call resolves to:
/// `Shape::area(self)`.
#[proc_macro_attribute]
pub fn methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as methods::MethodsInput);
    methods::expand(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Internal. Receives the full ancestor list from the `__obj_ancestors_*` chain.
#[doc(hidden)]
#[proc_macro]
pub fn __obj_emit(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as emit::EmitInput);
    let out = emit::expand(input).unwrap_or_else(syn::Error::into_compile_error);
    if std::env::var_os("OBJ_DUMP").is_some() {
        eprintln!("=== __obj_emit ===\n{out}\n");
    }
    out.into()
}
