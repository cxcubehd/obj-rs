# obj

C++-style inheritance for Rust — virtual functions, multiple inheritance, virtual base classes and
dynamic casts, with **zero per-object overhead**.

Rust deliberately has no implementation inheritance, and for almost every program that is the right
call. `obj` exists for the remainder: deep framework hierarchies, DOM/AST/widget trees, and ports of
C++ designs where the inheritance *is* the architecture.

## The idea

Rust already has vtables — `dyn Trait`. Rather than emitting a C++-style `__vptr` into every object
and reimplementing dispatch, `obj` maps each C++ concept onto machinery the compiler already
provides:

| C++ | `obj` | Cost |
|---|---|---|
| Field inheritance | `#[repr(C)]` base as first field, plus a `Deref` chain | zero |
| Virtual method | one generated trait per class; supertrait = base class | zero |
| `Derived*` → `Base*` | trait-upcasting coercion | zero |
| `dynamic_cast` | `ClassMeta` base table, reached by one virtual call | ~a call |
| Abstract class | the `Concrete` trait simply not implemented | compile-time |
| Virtual destructor | `Box<dyn _>` drop glue | free |
| Virtual base class | a byte offset into the complete object | one add |

Objects carry no hidden pointer: the vtable rides in the handle, exactly as it does for
`&dyn Trait`.

## The one rule

There are two ways to refer to an object, and they mean different things:

- `&Circle` — a **static view**. Calls resolve to inherent methods, non-virtually, like `obj.f()`
  in C++.
- `Obj<C>` / `Ref<C>` / `RefMut<C>` — a **polymorphic handle**. Calls dispatch virtually, like
  `ptr->f()` in C++.

Handles deref to the static view, so fields and inherited methods are always in reach. The reverse
needs a complete object, which is what makes C++ slicing unrepresentable.

## Two spellings

The attributes are the implementation:

```rust
#[obj::class(abstract)]
pub struct Shape { pub x: f64 }

#[obj::methods]
impl Shape {
    #[obj(virtual)] fn area(&self) -> f64;              // pure virtual
    #[obj(virtual)] fn scale(&mut self, k: f64) { .. }  // virtual with a default body
    fn position(&self) -> f64 { self.x }                // non-virtual, inherited via Deref
}

#[obj::class(extends(Shape, Drawable))]                 // multiple inheritance
pub struct Circle { pub r: f64 }

#[obj::methods]
impl Circle {
    #[obj(override)] fn area(&self) -> f64 { PI * self.r * self.r }
    #[obj(override)] fn scale(&mut self, k: f64) {
        self.r *= k;
        Shape::scale(self, k);      // a `super` call is a plain qualified call
    }
}
```

`obj::classes! { }` is sugar over exactly those attributes — one implementation, two surfaces —
and gives back the keywords, plus constructors with a base-initializer list:

```rust
obj::classes! {
    pub abstract class Shape dyn_traits(Debug) {
        pub x: f64,

        virtual fn area(&self) -> f64;
        virtual fn scale(&mut self, k: f64) { self.x *= k; }
        fn position(&self) -> f64 { self.x }
    }

    pub class Circle : Shape {
        pub r: f64,

        ctor new(x: f64, r: f64) : Shape { x } { r }

        override fn area(&self) -> f64 { PI * self.r * self.r }
    }
}
```

Either way you get the same thing:

```rust
let shapes: Vec<Obj<Shape>> = vec![
    Obj::<Circle>::new(Circle::new(0.0, 1.0)).upcast(),
    Obj::<Square>::new(Square::new(1.0, 2.0)).upcast(),
];
for s in &shapes {
    println!("{} {}", s.class().name, s.area());   // virtual dispatch
}

// dynamic_cast, downwards and sideways
let circle: Option<&Circle> = shapes[0].borrow().cast::<Circle>();
let drawable: Option<Ref<Drawable>> = shapes[0].borrow().cast_obj::<Drawable>();
```

## Virtual bases

Marking a base `virtual` makes it **shared**: however many paths reach it, the complete object
holds one copy. That is what deduplicates a diamond.

```rust
obj::classes! {
    pub abstract class InStream  : virtual Stream { pub reads: u32,  ctor new() { reads: 0 } }
    pub abstract class OutStream : virtual Stream { pub writes: u32, ctor new() { writes: 0 } }

    pub class Duplex : InStream, OutStream {
        pub name: String,

        // A most-derived constructor: the one that places the shared `Stream`.
        ctor new(name: &str, buffer: Vec<u8>)
            : InStream(), OutStream(), virtual Stream { buffer, pos: 0 }
            { name: name.into() }

        override fn kind(&self) -> &'static str { "duplex" }
    }
}
```

Reading and writing then move the *same* cursor. The shared base cannot live inside either branch,
so it lives in a generated complete-object type — `Class::Complete` — which is what
`Obj::<Duplex>::new` takes, and each subobject holds a byte offset to it. An offset rather than a
pointer, so the link survives the object being moved.

## Standard traits

`dyn_traits(..)` carries a trait across the whole hierarchy:

```rust
#[obj::class(abstract, dyn_traits(Debug, Clone, PartialEq, Eq, Hash))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Shape { pub x: i32 }
```

`Debug` and `Display` are object-safe, so they become supertraits of the class's interface.
`Clone`, `PartialEq`, `Eq` and `Hash` are not, so the macro generates object-safe shims:

- `Clone` gives `Obj<C>: Clone` and `clone_obj()` on every handle. The copy is of the
  **most-derived** class — the C++ virtual-clone idiom, not a slice.
- `PartialEq` compares heterogeneously: objects of different classes are never equal, so a
  `Rounded` whose `Square` prefix matches a real `Square` still is not one.
- `Hash` folds the class identity in first, keeping it consistent with that.

## Handles

| Handle | Owns | Analogue |
|---|---|---|
| `Obj<C>` | uniquely | `unique_ptr<C>` |
| `Shared<C>` / `WeakShared<C>` | by refcount | `shared_ptr<C>` / `weak_ptr<C>` |
| `ArcShared<C>` / `WeakArcShared<C>` | by atomic refcount, across threads | `shared_ptr<C>` |
| `Ref<'a, C>` / `RefMut<'a, C>` | borrows | `const C&` / `C&` |

A class may hold handles to other objects — a tree node owning `Vec<Obj<Node>>` is the normal case.
Such a class is not `Send`, so `ArcShared` is simply unavailable for it; everything else works.

## Performance

`cargo bench -p obj` measures dispatch against a hand-written `dyn Trait`:

```text
virtual dispatch, 20000 objects, best of 200 rounds

  dyn Trait (baseline)                 1.52 ns/call
  obj: Obj<Shape>                      1.52 ns/call
  obj: Ref<Shape>                      1.52 ns/call

per-object size

  Square (obj)                        16 bytes
  PlainSquare                         16 bytes
```

Dispatch and object size are identical — the vtable really does ride in the handle. One thing does
cost more: reading an *inherited field* through a base handle resolves the subobject offset through
the class metadata, about three times the cost of a hand-written accessor. Through a static
`&Circle` view it is a plain field access.

## Examples

```sh
cargo run -p obj --example shapes    # every feature, end to end
cargo run -p obj --example ast       # an expression tree, written with the DSL
cargo run -p obj --example diamond   # virtual bases: one shared base, two paths
```

## Status

Implemented and tested:

- single and multiple inheritance, to any depth
- virtual methods, pure virtuals, overrides and `super` calls
- abstract classes, uninstantiable at compile time
- virtual (shared) base classes, with diamond deduplication
- `dynamic_cast` downwards and sideways, preserving virtual dispatch
- upcasting, compile-time checked and free
- `Debug`, `Display`, `Clone`, `PartialEq`, `Eq` and `Hash` through a handle
- the `obj::classes! { }` DSL, including constructors with base-initializer lists
- all four handle families above
- `no_std` (with `alloc`), MSRV 1.86

Known limitations:

- A base class must live in the same crate as its subclasses, and its generated `XDyn` interface
  must be in scope where a subclass is declared.
- A hierarchy is capped at `MAX_BASES` (24) classes.
- A class may be inherited virtually or directly, but not both ways in one hierarchy.
- A non-virtual method that shares its name with an inherited virtual shadows it when the call is
  delegated through an abstract intermediate. Mark it `#[obj(override)]` instead.

## Safety

Three contained sources of `unsafe`, all in the runtime crate and documented at their definitions:
prefix-layout subobject addressing (guarded by `#[repr(C)]` and generated `offset_of!` assertions),
fat-pointer reconstruction for sidecasts, and virtual-base offset arithmetic. Generated code
contains one `unsafe` block, in the `complete(..)` constructor, where two `offset_of!` constants are
subtracted to form a virtual-base link.

All of it is covered by Miri. Hierarchies without virtual bases run under
`-Zmiri-strict-provenance`; virtual bases cannot, because reaching a shared base means addressing a
*sibling* of the subobject you hold and a reference grants permission for its own subobject only.
Those run under Tree Borrows instead.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
