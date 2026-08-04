//! `obj::classes! { }` — the C++-shaped surface.
//!
//! This is **sugar and nothing else**. Every declaration here expands to the same
//! `#[obj::class]` / `#[obj::methods]` pair you could have written by hand, so there is one
//! implementation of the object model and one place for it to be wrong.
//!
//! What the DSL buys is the parts that read badly as attributes: `virtual`, `override` and
//! `abstract` become real keywords again rather than `#[obj(..)]` markers, a base list reads
//! `: Shape, virtual Doc`, and — the reason it earns its place — constructors get a
//! base-initializer list, which is the only comfortable way to say what a virtual base is built
//! from.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Field, Ident, Token, TraitItemFn, Visibility};

use crate::common::*;

/// One or more class declarations.
pub struct Dsl {
    classes: Vec<ClassDecl>,
}

impl Parse for Dsl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut classes = Vec::new();
        while !input.is_empty() {
            classes.push(input.parse()?);
        }
        Ok(Dsl { classes })
    }
}

struct ClassDecl {
    attrs: Vec<Attribute>,
    vis: Visibility,
    is_abstract: bool,
    name: Ident,
    bases: Vec<BaseRef>,
    dyn_traits: Vec<Ident>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    ctors: Vec<Ctor>,
}

/// A method, carrying the `virtual` / `override` keyword the DSL lets you write literally.
struct Method {
    marker: Option<TokenStream>,
    item: TraitItemFn,
}

/// A constructor with a base-initializer list.
struct Ctor {
    attrs: Vec<Attribute>,
    vis: Visibility,
    name: Ident,
    params: TokenStream,
    inits: Vec<BaseInit>,
    /// The class's own field initialisers, as written between the final braces.
    body: TokenStream,
}

/// One entry of a base-initializer list: `Shape(x)` or `Shape { x }`.
struct BaseInit {
    is_virtual: bool,
    class: Ident,
    /// The whole initialising expression, already assembled.
    expr: TokenStream,
}

impl Parse for ClassDecl {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis: Visibility = input.parse()?;

        let is_abstract = input.peek(Token![abstract]);
        if is_abstract {
            input.parse::<Token![abstract]>()?;
        }

        let kw: Ident = input.parse()?;
        if kw != "class" {
            return Err(syn::Error::new(
                kw.span(),
                "obj: expected `class`; `obj::classes! { }` holds class declarations",
            ));
        }
        let name: Ident = input.parse()?;

        let mut bases = Vec::new();
        if input.peek(Token![:]) {
            input.parse::<Token![:]>()?;
            loop {
                // `dyn_traits(..)` is the only header item that is an identifier followed by a
                // parenthesis, so it can be told from a base class without lookahead tricks.
                if input.peek(Ident) && input.peek2(syn::token::Paren) {
                    break;
                }
                bases.push(input.parse::<BaseRef>()?);
                if input.peek(Token![,]) {
                    input.parse::<Token![,]>()?;
                } else {
                    break;
                }
            }
        }

        let mut dyn_traits: Vec<Ident> = Vec::new();
        if input.peek(Ident) {
            let key: Ident = input.parse()?;
            if key != "dyn_traits" {
                return Err(syn::Error::new(
                    key.span(),
                    "obj: expected `dyn_traits(..)` or the class body",
                ));
            }
            let inner;
            syn::parenthesized!(inner in input);
            dyn_traits = Punctuated::<Ident, Token![,]>::parse_terminated(&inner)?
                .into_iter()
                .collect();
        }

        let body;
        syn::braced!(body in input);

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut ctors = Vec::new();
        while !body.is_empty() {
            let attrs = body.call(Attribute::parse_outer)?;

            // A constructor: `ctor` is an ordinary identifier, so it is only one when a name
            // follows. `ctor: T` is a field called `ctor`.
            if body.peek(Ident) && body.peek2(Ident) {
                let kw: Ident = body.fork().parse()?;
                if kw == "ctor" {
                    body.parse::<Ident>()?;
                    ctors.push(parse_ctor(&body, attrs, Visibility::Inherited)?);
                    continue;
                }
            }

            let vis: Visibility = body.parse()?;

            if body.peek(Ident) && body.peek2(Ident) {
                let kw: Ident = body.fork().parse()?;
                if kw == "ctor" {
                    body.parse::<Ident>()?;
                    ctors.push(parse_ctor(&body, attrs, vis)?);
                    continue;
                }
            }

            let marker = if body.peek(Token![virtual]) {
                body.parse::<Token![virtual]>()?;
                Some(quote!(#[obj(virtual)]))
            } else if body.peek(Token![override]) {
                body.parse::<Token![override]>()?;
                Some(quote!(#[obj(override)]))
            } else {
                None
            };

            if marker.is_some() || body.peek(Token![fn]) {
                let mut item: TraitItemFn = body.parse()?;
                item.attrs = attrs;
                methods.push(Method { marker, item });
                continue;
            }

            // Anything left is a field.
            let mut field = Field::parse_named(&body)?;
            field.attrs = attrs;
            field.vis = vis;
            fields.push(field);
            if body.peek(Token![,]) {
                body.parse::<Token![,]>()?;
            }
        }

        Ok(ClassDecl {
            attrs,
            vis,
            is_abstract,
            name,
            bases,
            dyn_traits,
            fields,
            methods,
            ctors,
        })
    }
}

fn parse_ctor(input: ParseStream, attrs: Vec<Attribute>, vis: Visibility) -> syn::Result<Ctor> {
    let name: Ident = input.parse()?;
    let args;
    syn::parenthesized!(args in input);
    let params: TokenStream = args.parse()?;

    let mut inits = Vec::new();
    if input.peek(Token![:]) {
        input.parse::<Token![:]>()?;
        loop {
            let is_virtual = input.peek(Token![virtual]);
            if is_virtual {
                input.parse::<Token![virtual]>()?;
            }
            let class: Ident = input.parse()?;
            // `Shape(x)` calls the base's own constructor; `Shape { x }` writes it out. The list
            // is comma-separated and the body is not, so a braced initialiser cannot be mistaken
            // for the body that follows it.
            let expr = if input.peek(syn::token::Paren) {
                let call;
                syn::parenthesized!(call in input);
                let call: TokenStream = call.parse()?;
                quote!(#class::new(#call))
            } else if input.peek(syn::token::Brace) {
                let lit;
                syn::braced!(lit in input);
                let lit: TokenStream = lit.parse()?;
                quote!(#class { #lit })
            } else {
                return Err(syn::Error::new(
                    class.span(),
                    format!(
                        "obj: write `{class}(..)` to call `{class}::new`, or `{class} {{ .. }}` to \
                         build it directly",
                    ),
                ));
            };
            inits.push(BaseInit {
                is_virtual,
                class,
                expr,
            });
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }
    }

    let body;
    syn::braced!(body in input);
    let body: TokenStream = body.parse()?;

    Ok(Ctor {
        attrs,
        vis,
        name,
        params,
        inits,
        body,
    })
}

pub fn expand(dsl: Dsl) -> syn::Result<TokenStream> {
    let mut out = TokenStream::new();
    for class in &dsl.classes {
        out.extend(expand_class(class)?);
    }
    Ok(out)
}

fn expand_class(class: &ClassDecl) -> syn::Result<TokenStream> {
    let ClassDecl {
        attrs,
        vis,
        is_abstract,
        name,
        bases,
        dyn_traits,
        fields,
        methods,
        ctors,
    } = class;

    // Rebuild the attribute form. Everything below this point is exactly what a user writing the
    // attributes by hand would produce.
    let mut args = Vec::new();
    if *is_abstract {
        args.push(quote!(abstract));
    }
    if !bases.is_empty() {
        args.push(quote!(extends(#(#bases),*)));
    }
    if !dyn_traits.is_empty() {
        args.push(quote!(dyn_traits(#(#dyn_traits),*)));
    }
    let class_attr = if args.is_empty() {
        quote!(#[::obj::class])
    } else {
        quote!(#[::obj::class(#(#args),*)])
    };

    let method_items = methods.iter().map(|m| {
        let Method { marker, item } = m;
        quote!(#marker #item)
    });

    let ctor_items = ctors
        .iter()
        .map(|c| expand_ctor(class, c))
        .collect::<syn::Result<Vec<_>>>()?;

    let ctor_block = (!ctor_items.is_empty()).then(|| {
        quote! {
            impl #name {
                #(#ctor_items)*
            }
        }
    });

    // `#[obj::class]` goes first so it expands first. Attributes run in order, and a `#[derive]`
    // placed above it would see the struct before the base subobjects were injected — generating,
    // say, a `Clone` that forgets to copy them. Writing the attributes by hand carries the same
    // rule; here the DSL just applies it for you, whatever order they were written in.
    Ok(quote! {
        #class_attr
        #(#attrs)*
        #vis struct #name {
            #(#fields,)*
        }

        #[::obj::methods]
        impl #name {
            #(#method_items)*
        }

        #ctor_block
    })
}

fn expand_ctor(class: &ClassDecl, ctor: &Ctor) -> syn::Result<TokenStream> {
    let Ctor {
        attrs,
        vis,
        name,
        params,
        inits,
        body,
    } = ctor;
    let class_name = &class.name;

    let (shared, stored): (Vec<&BaseInit>, Vec<&BaseInit>) =
        inits.iter().partition(|i| i.is_virtual);

    // An abstract class may still have a constructor — it builds the *subobject*, which is exactly
    // what a derived class's initialiser list needs. What it cannot do is build a complete object,
    // which is what listing shared bases asks for.
    if class.is_abstract && !shared.is_empty() {
        return Err(syn::Error::new(
            name.span(),
            format!(
                "obj: `{class_name}` is abstract, so it is never the most-derived class and must \
                 not initialise virtual bases; leave that to the concrete class below it",
            ),
        ));
    }

    // Every base stored inside this class has to be initialised, and only a base this class
    // actually has may be. Catching both here beats a missing-field error pointing at expanded
    // code the user never wrote.
    for init in &stored {
        if !class
            .bases
            .iter()
            .any(|b| !b.is_virtual && b.class == init.class)
        {
            return Err(syn::Error::new(
                init.class.span(),
                format!(
                    "obj: `{class_name}` does not have `{}` as a direct base",
                    init.class
                ),
            ));
        }
    }
    for base in class.bases.iter().filter(|b| !b.is_virtual) {
        if !stored.iter().any(|i| i.class == base.class) {
            return Err(syn::Error::new(
                name.span(),
                format!(
                    "obj: constructor `{name}` does not initialise the `{}` base of \
                     `{class_name}`; add `{}(..)` to its initialiser list",
                    base.class, base.class,
                ),
            ));
        }
    }

    let stored_fields = stored.iter().map(|i| {
        let (f, e) = (base_field(&i.class), &i.expr);
        quote!(#f: #e,)
    });
    // A virtual base is not stored here, only linked, and the link is written by `complete(..)`.
    let vbase_slots = class.bases.iter().filter(|b| b.is_virtual).map(|b| {
        let f = base_field(&b.class);
        quote!(#f: ::obj::VBase::new(),)
    });

    let subobject = quote! {
        #class_name {
            #(#stored_fields)*
            #(#vbase_slots)*
            #body
        }
    };

    if shared.is_empty() {
        return Ok(quote! {
            #(#attrs)*
            #vis fn #name(#params) -> #class_name { #subobject }
        });
    }

    // Listing shared bases makes this a most-derived constructor: it produces the whole complete
    // object, placing each shared base once, exactly as the C++ rule that only the most-derived
    // constructor initialises virtual bases.
    let shared_exprs = shared.iter().map(|i| &i.expr);
    Ok(quote! {
        #(#attrs)*
        #vis fn #name(#params) -> <#class_name as ::obj::Class>::Complete {
            #class_name::complete(#subobject #(, #shared_exprs)*)
        }
    })
}
