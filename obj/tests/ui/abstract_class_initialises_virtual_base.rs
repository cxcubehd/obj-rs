//! Only the most-derived class initialises virtual bases, and an abstract class is never it.
obj::classes! {
    pub abstract class Doc {
        pub id: u32,
        virtual fn render(&self) -> u8;
    }

    pub abstract class Html : virtual Doc {
        pub tag: u8,

        // `Html` is abstract, so it can never be the most-derived class.
        ctor new(id: u32, tag: u8) : virtual Doc { id } { tag }
    }
}

fn main() {}
