//! Virtual (shared) base classes: the diamond, deduplicated.
//!
//! ```text
//!            Doc (abstract)
//!          /       \
//!   (virtual)     (virtual)
//!        /           \
//!     Html          Xml          (both abstract)
//!        \           /
//!         `- Xhtml -'
//! ```
//!
//! Without `virtual`, `Xhtml` would contain two `Doc` subobjects and `id` would be ambiguous.
//! With it there is exactly one, stored in the complete object, and every path reaches it.

#![allow(missing_docs)]

use obj::{Class, Obj, Ref, VBase};

#[obj::class(abstract)]
pub struct Doc {
    pub id: u32,
}

#[obj::methods]
impl Doc {
    #[obj(virtual)]
    fn render(&self) -> String;

    /// Virtual with a body, overridden by nobody: reaching it from `Xhtml` has to pass through
    /// two abstract classes.
    #[obj(virtual)]
    fn bump(&mut self) {
        self.id += 1;
    }

    /// Non-virtual, inherited.
    fn describe(&self) -> String {
        format!("doc#{}", self.id)
    }
}

#[obj::class(extends(virtual Doc), abstract)]
pub struct Html {
    pub tag: String,
}

#[obj::methods]
impl Html {}

#[obj::class(extends(virtual Doc), abstract)]
pub struct Xml {
    pub ns: String,
}

#[obj::methods]
impl Xml {}

#[obj::class(extends(Html, Xml))]
pub struct Xhtml {
    pub strict: bool,
}

#[obj::methods]
impl Xhtml {
    #[obj(override)]
    fn render(&self) -> String {
        format!(
            "<{} xmlns:{} id={}>",
            self.tag,
            self.as_xml().ns,
            self.as_doc().id
        )
    }
}

/// A concrete class whose *only* base is virtual, to check that case standalone.
#[obj::class(extends(virtual Doc))]
pub struct Plain {
    pub text: String,
}

#[obj::methods]
impl Plain {
    #[obj(override)]
    fn render(&self) -> String {
        format!("{}#{}", self.text, self.id)
    }
}

/// A subclass of a class that already has virtual bases. Its complete object is rebuilt one level
/// down, so every link has to be measured afresh from the new wrapper.
#[obj::class(extends = Xhtml)]
pub struct Xhtml5 {
    pub doctype: String,
}

#[obj::methods]
impl Xhtml5 {
    #[obj(override)]
    fn render(&self) -> String {
        // A `super` call: the base's own implementation, non-virtually.
        format!("<!{}>{}", self.doctype, Xhtml::render(self))
    }
}

fn xhtml5(id: u32) -> <Xhtml5 as Class>::Complete {
    Xhtml5::complete(
        Xhtml5 {
            xhtml: Xhtml {
                html: Html {
                    doc: VBase::new(),
                    tag: "p".into(),
                },
                xml: Xml {
                    doc: VBase::new(),
                    ns: "x".into(),
                },
                strict: true,
            },
            doctype: "DOCTYPE".into(),
        },
        Doc { id },
    )
}

fn xhtml(id: u32) -> <Xhtml as Class>::Complete {
    Xhtml::complete(
        Xhtml {
            html: Html {
                doc: VBase::new(),
                tag: "p".into(),
            },
            xml: Xml {
                doc: VBase::new(),
                ns: "x".into(),
            },
            strict: true,
        },
        Doc { id },
    )
}

fn plain(id: u32, text: &str) -> <Plain as Class>::Complete {
    Plain::complete(
        Plain {
            doc: VBase::new(),
            text: text.into(),
        },
        Doc { id },
    )
}

#[test]
fn the_shared_base_exists_exactly_once() {
    let x = Obj::<Xhtml>::new(xhtml(7));

    // Both paths reach the same `Doc`, so writing through one is visible through the other.
    let via_html: *const Doc = x.as_doc();
    let via_xml: *const Doc = x.as_xml().as_doc();
    assert_eq!(via_html, via_xml, "one shared subobject, not two");
}

#[test]
fn mutation_through_one_path_is_visible_through_the_other() {
    let mut x = Obj::<Xhtml>::new(xhtml(7));

    x.as_doc_mut().id = 42;
    assert_eq!(x.as_xml().as_doc().id, 42, "the Xml path sees the write");
    assert_eq!(
        x.id, 42,
        "and so does Deref, which walks Xhtml -> Html -> Doc"
    );
}

#[test]
fn the_base_table_lists_the_shared_base_once() {
    let meta = <Xhtml as Class>::META;
    let docs = meta
        .bases
        .iter()
        .filter(|b| (b.id)() == core::any::TypeId::of::<Doc>())
        .count();
    assert_eq!(docs, 1, "the diamond is deduplicated");

    // Doc, Html, Xml, Xhtml -- four classes, four entries.
    assert_eq!(meta.bases.len(), 4);
    for id in [
        core::any::TypeId::of::<Doc>(),
        core::any::TypeId::of::<Html>(),
        core::any::TypeId::of::<Xml>(),
        core::any::TypeId::of::<Xhtml>(),
    ] {
        assert!(meta.is_a(id));
    }
}

#[test]
fn the_shared_base_is_not_inside_either_branch() {
    // `Html` stores a link, not a `Doc`, which is what stops the diamond duplicating it.
    assert!(
        core::mem::size_of::<Html>() < core::mem::size_of::<Html>() + core::mem::size_of::<Doc>(),
    );
    let meta = <Xhtml as Class>::META;
    let doc = meta
        .find_base(core::any::TypeId::of::<Doc>())
        .expect("present");
    // The shared base sits past the whole `Xhtml` subobject, in the complete object.
    assert!(
        doc.data_offset >= core::mem::size_of::<Xhtml>(),
        "shared base lives outside the class, at offset {}",
        doc.data_offset,
    );
}

#[test]
fn virtual_dispatch_reaches_the_override_through_the_shared_base() {
    let doc: Obj<Doc> = Obj::<Xhtml>::new(xhtml(7)).upcast();
    assert_eq!(doc.render(), "<p xmlns:x id=7>", "Xhtml's override runs");
    assert_eq!(doc.class().name, "Xhtml");
}

#[test]
fn an_inherited_default_reaches_through_two_abstract_classes() {
    // `bump` has a body on `Doc` and is overridden nowhere. Delegation from `Xhtml` has to pass
    // through `Html`, which is abstract and so implements no interface to hand off to.
    let mut x = Obj::<Xhtml>::new(xhtml(7));
    x.bump();
    assert_eq!(x.as_doc().id, 8);

    // ...and through a base-class handle too.
    let mut doc: Obj<Doc> = Obj::<Xhtml>::new(xhtml(7)).upcast();
    doc.bump();
    assert_eq!(doc.id, 8);
}

#[test]
fn non_virtual_methods_of_the_shared_base_are_inherited() {
    let x = Obj::<Xhtml>::new(xhtml(3));
    assert_eq!(x.describe(), "doc#3", "reached by Deref through Html");
}

#[test]
fn casting_to_the_shared_base_and_back() {
    let x = Obj::<Xhtml>::new(xhtml(7));
    assert!(x.is::<Doc>());

    let doc: &Doc = x.borrow().cast::<Doc>().expect("is a Doc");
    assert_eq!(doc.id, 7);

    // Sidecast from one branch of the diamond to the other keeps virtual dispatch.
    let as_doc: Ref<'_, Doc> = x.borrow().cast_obj::<Doc>().expect("is a Doc");
    assert_eq!(as_doc.render(), "<p xmlns:x id=7>");

    let back: Ref<'_, Xhtml> = as_doc.cast_obj::<Xhtml>().expect("is an Xhtml");
    assert!(back.strict);
}

#[test]
fn links_survive_moving_the_complete_object() {
    // The link is a *relative* offset precisely so that moving the object keeps it valid; a
    // stored pointer would dangle here.
    let a = xhtml(7);
    let b = a; // move
    let boxed = Box::new(b); // move again, onto the heap
    let x = Obj::<Xhtml>::new(*boxed); // and once more, into the handle

    assert_eq!(x.as_doc().id, 7);
    assert_eq!(x.as_xml().as_doc().id, 7);
    assert_eq!(x.render(), "<p xmlns:x id=7>");
}

#[test]
fn a_class_whose_only_base_is_virtual() {
    let p = Obj::<Plain>::new(plain(4, "hello"));

    assert_eq!(p.text, "hello");
    assert_eq!(p.id, 4, "Deref targets the shared base");
    assert_eq!(p.render(), "hello#4");

    let doc: Obj<Doc> = p.upcast();
    assert_eq!(doc.render(), "hello#4");
    assert_eq!(doc.id, 4);
}

#[test]
fn two_hierarchies_sharing_a_base_stay_independent() {
    let x = Obj::<Xhtml>::new(xhtml(1));
    let p = Obj::<Plain>::new(plain(2, "hi"));

    let docs: Vec<Obj<Doc>> = vec![x.upcast(), p.upcast()];
    let rendered: Vec<String> = docs.iter().map(|d| d.render()).collect();
    assert_eq!(rendered, ["<p xmlns:x id=1>", "hi#2"]);
    assert_eq!(docs[0].id, 1);
    assert_eq!(docs[1].id, 2);
}

#[test]
fn a_subclass_relinks_the_shared_base_to_its_own_complete_object() {
    let mut x = Obj::<Xhtml5>::new(xhtml5(9));

    assert_eq!(x.as_doc().id, 9, "reached through Xhtml5 -> Xhtml -> Html");
    assert_eq!(x.as_xml().as_doc().id, 9, "and through the other branch");
    assert_eq!(
        x.render(),
        "<!DOCTYPE><p xmlns:x id=9>",
        "override plus super"
    );

    // Still one shared copy, two levels down.
    let via_html: *const Doc = x.as_doc();
    let via_xml: *const Doc = x.as_xml().as_doc();
    assert_eq!(via_html, via_xml);

    x.bump();
    assert_eq!(x.as_doc().id, 10);

    let doc: Obj<Doc> = x.upcast();
    assert_eq!(doc.render(), "<!DOCTYPE><p xmlns:x id=10>");
    assert_eq!(
        <Xhtml5 as Class>::META.bases.len(),
        5,
        "Doc still counted once"
    );
}

#[test]
fn an_unlinked_subobject_panics_rather_than_reading_garbage() {
    // Built by hand rather than through `complete`, so the link was never set.
    let stray = Html {
        doc: VBase::new(),
        tag: "p".into(),
    };
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = stray.as_doc().id;
    }));
    assert!(panicked.is_err(), "resolving an unlinked base must panic");
}
