//! A class with a pure virtual method must not be constructible.
use obj::Obj;

#[obj::class(abstract)]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    fn area(&self) -> f64;
}

fn main() {
    let _ = Obj::<Shape>::new(Shape { x: 1.0 });
}
