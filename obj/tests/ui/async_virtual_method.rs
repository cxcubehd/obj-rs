//! `async` virtual methods are not object-safe.
#[obj::class]
pub struct Shape {
    pub x: f64,
}

#[obj::methods]
impl Shape {
    #[obj(virtual)]
    async fn load(&self) {}
}

fn main() {}
