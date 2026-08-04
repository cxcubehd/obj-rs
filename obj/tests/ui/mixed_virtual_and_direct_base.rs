//! A class must be inherited virtually everywhere, or nowhere: mixing the two would put two
//! copies of it in the object under one name.
#[obj::class(abstract)]
pub struct Doc {
    pub id: u32,
}

#[obj::methods]
impl Doc {}

#[obj::class(extends(virtual Doc), abstract)]
pub struct Html {
    pub tag: u8,
}

#[obj::methods]
impl Html {}

// `Doc` arrives virtually through `Html` and directly here.
#[obj::class(extends(Html, Doc))]
pub struct Xhtml {
    pub strict: bool,
}

#[obj::methods]
impl Xhtml {}

fn main() {}
