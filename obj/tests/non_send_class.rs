//! A class may hold anything, including handles to other objects.
//!
//! Tree-shaped hierarchies are the main thing `obj` is for, and a tree node holds its children —
//! so a class field is routinely an `Obj<Node>`, which is neither `Send` nor `Sync`. That has to
//! be expressible: the thread-safe handle simply becomes unavailable for such a class, rather
//! than the class failing to compile.

#![allow(missing_docs)]

use obj::{ArcShared, Class, Obj, Shared};

#[obj::class(abstract)]
pub struct Node {
    pub id: u32,
}

#[obj::methods]
impl Node {
    #[obj(virtual)]
    fn total(&self) -> u32;
}

#[obj::class(extends = Node)]
pub struct Leaf {
    pub value: u32,
}

#[obj::methods]
impl Leaf {
    #[obj(override)]
    fn total(&self) -> u32 {
        self.value
    }
}

/// Holds `Obj` children, so `Branch` is not `Send`.
#[obj::class(extends = Node)]
pub struct Branch {
    pub children: Vec<Obj<Node>>,
}

#[obj::methods]
impl Branch {
    #[obj(override)]
    fn total(&self) -> u32 {
        self.children.iter().map(|c| c.total()).sum()
    }
}

/// A plain-data class, which *is* thread-safe.
#[obj::class(extends = Node)]
pub struct Counter {
    pub count: u32,
}

#[obj::methods]
impl Counter {
    #[obj(override)]
    fn total(&self) -> u32 {
        self.count
    }
}

fn leaf(id: u32, value: u32) -> Obj<Node> {
    Obj::<Leaf>::new(Leaf {
        node: Node { id },
        value,
    })
    .upcast()
}

#[test]
fn a_class_can_own_handles_to_other_objects() {
    let tree = Obj::<Branch>::new(Branch {
        node: Node { id: 0 },
        children: vec![
            leaf(1, 10),
            leaf(2, 20),
            Obj::<Branch>::new(Branch {
                node: Node { id: 3 },
                children: vec![leaf(4, 30), leaf(5, 40)],
            })
            .upcast(),
        ],
    });

    assert_eq!(tree.total(), 100, "recursive virtual dispatch");
    assert_eq!(tree.children.len(), 3);
    assert_eq!(tree.id, 0, "inherited field");

    // The whole hierarchy still works through a base handle.
    let node: Obj<Node> = tree.upcast();
    assert_eq!(node.total(), 100);
    assert!(node.is::<Branch>());
}

#[test]
fn the_single_threaded_handles_still_work_for_such_a_class() {
    let shared = Shared::<Branch>::new(Branch {
        node: Node { id: 0 },
        children: vec![leaf(1, 7)],
    });
    let alias = shared.clone();

    assert_eq!(shared.total(), 7);
    assert_eq!(alias.strong_count(), 2);
}

#[test]
fn the_thread_safe_handle_is_available_for_a_thread_safe_class() {
    // `Counter` holds only plain data, so it keeps `ArcShared` -- the bound rides on the value,
    // not on the class declaration.
    let arc: ArcShared<Node> = ArcShared::<Counter>::new(Counter {
        node: Node { id: 1 },
        count: 5,
    })
    .upcast();

    let worker = {
        let arc = arc.clone();
        std::thread::spawn(move || arc.total())
    };
    assert_eq!(worker.join().expect("worker panicked"), 5);
    assert_eq!(<Counter as Class>::META.name, "Counter");
}
