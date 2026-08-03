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

## Example

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
    #[obj(override)] fn area(&self) -> f64 {
        PI * self.r * self.r
    }
    #[obj(override)] fn scale(&mut self, k: f64) {
        self.r *= k;
        Shape::scale(self, k);      // a `super` call is a plain qualified call
    }
}

let shapes: Vec<Obj<Shape>> = vec![
    Obj::<Circle>::new(Circle::new(1.0)).upcast(),
    Obj::<Square>::new(Square::new(2.0)).upcast(),
];
for s in &shapes {
    println!("{} {}", s.class().name, s.area());   // virtual dispatch
}

// dynamic_cast, downwards and sideways
let circle: Option<&Circle> = shapes[0].borrow().cast::<Circle>();
let drawable: Option<Ref<Drawable>> = shapes[0].borrow().cast_obj::<Drawable>();
```

`cargo run -p obj --example shapes` runs a worked example of all of the above.

## Handles

| Handle | Owns | Analogue |
|---|---|---|
| `Obj<C>` | uniquely | `unique_ptr<C>` |
| `Shared<C>` / `WeakShared<C>` | by refcount | `shared_ptr<C>` / `weak_ptr<C>` |
| `ArcShared<C>` / `WeakArcShared<C>` | by atomic refcount, across threads | `shared_ptr<C>` |
| `Ref<'a, C>` / `RefMut<'a, C>` | borrows | `const C&` / `C&` |

## Status

Implemented and tested:

- single and multiple inheritance, to any depth
- virtual methods, pure virtuals, overrides and `super` calls
- abstract classes, uninstantiable at compile time
- `dynamic_cast` downwards and sideways, preserving virtual dispatch
- upcasting, compile-time checked and free
- all four handle families above
- `no_std` (with `alloc`), MSRV 1.86

Not yet implemented: virtual (shared) base classes for diamond
deduplication, the `obj::class! {}` DSL sugar, and built-in derives for
standard traits such as `Debug` and `Clone`.

Known limitations: a base class must currently live in the same crate as
its subclasses, and a hierarchy is capped at `MAX_BASES` (24) classes.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
