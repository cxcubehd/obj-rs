//! Procedural macros for the [`obj`](https://docs.rs/obj) crate.
//!
//! These are re-exported by `obj`; depend on that crate rather than this one.

use proc_macro::TokenStream;
use syn::parse_macro_input;

mod class;
mod common;
mod dsl;
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
///
/// #[obj::class(extends(Html, Xml))]   // multiple inheritance
/// pub struct Xhtml { }
/// ```
///
/// # Virtual bases
///
/// Marking a base `virtual` makes it **shared**: however many paths through the hierarchy reach
/// it, the complete object holds one copy. This is what deduplicates a diamond.
///
/// ```ignore
/// #[obj::class(extends(virtual Doc))] pub struct Html { pub tag: String }
/// #[obj::class(extends(virtual Doc))] pub struct Xml  { pub ns: String }
/// #[obj::class(extends(Html, Xml))]   pub struct Xhtml { pub strict: bool }
/// ```
///
/// The class then stores a `VBase` link where a subobject would otherwise sit, so
/// write `VBase::new()` for it in a struct literal and build the object through the generated
/// `complete(..)` constructor, which places each shared base once and links every subobject to it:
///
/// ```ignore
/// let obj = Obj::<Xhtml>::new(Xhtml::complete(
///     Xhtml {
///         html: Html { doc: VBase::new(), tag: "p".into() },
///         xml:  Xml  { doc: VBase::new(), ns:  "x".into() },
///         strict: true,
///     },
///     Doc { id: 1 },      // the one shared base
/// ));
/// ```
///
/// Reach it with the generated `as_doc()` / `as_doc_mut()`, or through `Deref` when the virtual
/// base is the class's only base. This mirrors C++, where the most-derived constructor is likewise
/// the only one that initialises virtual bases.
///
/// The struct is given `#[repr(C)]` and, when it has a base, the base subobject is inserted as its
/// first field. Every class must also have an `#[obj::methods]` block, which declares its virtual
/// methods; write an empty one if it has none.
///
/// # Standard traits
///
/// `dyn_traits(..)` carries a standard trait through every handle to the class, and down to every
/// subclass:
///
/// ```ignore
/// #[obj::class(abstract, dyn_traits(Debug, Clone, PartialEq, Eq, Hash))]
/// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// pub struct Shape { pub x: i32 }
/// ```
///
/// `Debug` and `Display` are object-safe, so they simply become supertraits of the class's
/// interface. `Clone`, `PartialEq`, `Eq` and `Hash` are not, so the macro generates the
/// object-safe shims in [`obj::dyn_traits`](../obj/dyn_traits/index.html) instead:
///
/// - `Clone` gives `Obj<C>: Clone` and a `clone_obj()` on every handle. The copy is of the
///   **most-derived** class, which is the C++ "virtual clone" idiom.
/// - `PartialEq` compares heterogeneously: objects of different classes are never equal.
/// - `Hash` folds the class identity in first, so it agrees with that rule.
///
/// The class must implement the trait itself — usually by `#[derive]`, which must be written
/// *below* `#[obj::class]` so it sees the injected base field.
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

/// Declares whole classes in one block, C++-style.
///
/// ```ignore
/// obj::classes! {
///     pub abstract class Shape dyn_traits(Debug) {
///         pub x: f64,
///
///         virtual fn area(&self) -> f64;                    // pure virtual
///         virtual fn scale(&mut self, k: f64) { self.x *= k; }
///         fn position(&self) -> f64 { self.x }              // non-virtual
///     }
///
///     pub class Circle : Shape, virtual Drawable {
///         pub r: f64,
///
///         ctor new(x: f64, r: f64) : Shape(x), virtual Drawable(true) { r }
///
///         override fn area(&self) -> f64 { PI * self.r * self.r }
///     }
/// }
/// ```
///
/// This is sugar: it expands to the [`class`] and [`methods`] attributes and nothing else, so both
/// spellings produce identical code and there is only one implementation to trust. What it buys is
/// the parts that read badly as attributes — `virtual`, `override` and `abstract` as real keywords,
/// a base list after `:`, and constructors with a base-initializer list.
///
/// # Constructors
///
/// `ctor <name>(<params>) : <base initialisers> { <own fields> }` becomes an inherent function.
/// Each initialiser is either `Base(args)`, which calls that base's own `new`, or `Base { .. }`,
/// which writes it out. The braces at the end hold this class's own fields, exactly as in a struct
/// literal.
///
/// Marking an initialiser `virtual` makes the constructor a *most-derived* one: it returns the
/// complete object with each shared base placed once, mirroring the C++ rule that only the
/// most-derived constructor initialises virtual bases. List shared bases in the order they appear
/// in the hierarchy.
#[proc_macro]
pub fn classes(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as dsl::Dsl);
    dsl::expand(input)
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
