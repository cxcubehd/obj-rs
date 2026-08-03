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

## Status

Under active development. See `PLAN` in the repository history for the phase breakdown.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
