//! The DSL's reason for existing: constructors with base-initializer lists.
//!
//! Virtual bases are where writing this out by hand hurts most — the shared base is built by the
//! most-derived class, every subobject holds a link to it, and a struct literal has to mention
//! both. A `ctor` says it once.

#![allow(missing_docs)]

use obj::{Class, Obj};

obj::classes! {
    pub abstract class Doc dyn_traits(Debug, Clone, PartialEq) {
        pub id: u32,

        virtual fn render(&self) -> String;

        virtual fn bump(&mut self) {
            self.id += 1;
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub abstract class Html : virtual Doc {
        pub tag: String,

        // A constructor on an abstract class builds its *subobject*, which is what a derived
        // class's initialiser list needs. The `Doc` link is filled in for us.
        ctor new(tag: &str) { tag: tag.into() }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub abstract class Xml : virtual Doc {
        pub ns: String,

        ctor new(ns: &str) { ns: ns.into() }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub class Xhtml : Html, Xml {
        pub strict: bool,

        /// A most-derived constructor: it names the shared base, so it yields a complete object.
        ctor new(id: u32, tag: &str, ns: &str)
            : Html(tag), Xml(ns), virtual Doc { id }
            { strict: true }

        override fn render(&self) -> String {
            format!("<{} xmlns:{} id={}>", self.tag, self.as_xml().ns, self.as_doc().id)
        }
    }
}

// `Doc` needs the derives too, and the DSL passes attributes through, but it is declared above
// without them so that this file also proves an attribute-free class header parses.
impl core::fmt::Debug for Doc {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Doc {{ id: {} }}", self.id)
    }
}

impl Clone for Doc {
    fn clone(&self) -> Self {
        Doc { id: self.id }
    }
}

impl PartialEq for Doc {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[test]
fn a_most_derived_ctor_yields_a_complete_object() {
    // `Xhtml::new` returns the complete object, so it drops straight into `Obj::new`.
    let x = Obj::<Xhtml>::new(Xhtml::new(7, "p", "xh"));

    assert_eq!(x.tag, "p", "set through the Html initialiser");
    assert_eq!(x.as_xml().ns, "xh", "and the Xml one");
    assert!(x.strict, "own field");
    assert_eq!(x.as_doc().id, 7, "the shared base, built once");
    assert_eq!(x.render(), "<p xmlns:xh id=7>");
}

#[test]
fn the_ctor_links_every_path_to_one_shared_base() {
    let x = Obj::<Xhtml>::new(Xhtml::new(1, "p", "xh"));

    let via_html: *const Doc = x.as_doc();
    let via_xml: *const Doc = x.as_xml().as_doc();
    assert_eq!(via_html, via_xml, "one Doc, reached two ways");

    assert_eq!(<Xhtml as Class>::META.bases.len(), 4, "Doc counted once");
}

#[test]
fn the_shared_base_is_writable_through_either_path() {
    let mut x = Obj::<Xhtml>::new(Xhtml::new(1, "p", "xh"));

    x.bump(); // Doc's own body, reached through two abstract classes
    assert_eq!(x.as_doc().id, 2);
    assert_eq!(x.as_xml().as_doc().id, 2);
}

#[test]
fn dyn_traits_reach_the_complete_object() {
    let a: Obj<Doc> = Obj::<Xhtml>::new(Xhtml::new(1, "p", "xh")).upcast();
    let same: Obj<Doc> = Obj::<Xhtml>::new(Xhtml::new(1, "p", "xh")).upcast();
    let other: Obj<Doc> = Obj::<Xhtml>::new(Xhtml::new(2, "p", "xh")).upcast();

    assert_eq!(a, same);
    assert_ne!(a, other, "the shared base is part of the value");

    let copy = a.clone();
    assert_eq!(copy.render(), "<p xmlns:xh id=1>", "cloned and relinked");
    assert_eq!(copy, a);
}
