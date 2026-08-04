//! Virtual base classes: one shared base, however many paths reach it.
//!
//! The classic diamond. A GUI stream can be read, written, or both — and a read-write stream must
//! have exactly one buffer, not one per side.
//!
//! ```text
//!             Stream (abstract)
//!            /                \
//!      (virtual)            (virtual)
//!          /                    \
//!    InStream (abstract)   OutStream (abstract)
//!           \                  /
//!            `--- Duplex -----'
//! ```
//!
//! Without `virtual`, `Duplex` would contain two `Stream` subobjects, `pos` would be ambiguous,
//! and reading would advance a different cursor from the one writing advanced. With it there is
//! one `Stream`, stored in the complete object, and both sides share it.
//!
//! Run with `cargo run -p obj --example diamond`.

#![allow(missing_docs)]

use obj::{Class, Obj};

obj::classes! {
    /// What every stream has: a cursor into one shared buffer.
    pub abstract class Stream {
        pub buffer: Vec<u8>,
        pub pos: usize,

        /// Pure virtual: each stream kind names itself.
        virtual fn kind(&self) -> &'static str;

        /// Virtual with a body, overridden by nobody below — reaching it from `Duplex` has to
        /// pass through two abstract classes.
        virtual fn remaining(&self) -> usize {
            self.buffer.len() - self.pos
        }
    }

    /// The reading half. `Stream` is virtual, so this stores a *link* to it, not a copy.
    pub abstract class InStream : virtual Stream {
        pub reads: u32,

        ctor new() { reads: 0 }
    }

    /// The writing half, sharing the very same `Stream`.
    pub abstract class OutStream : virtual Stream {
        pub writes: u32,

        ctor new() { writes: 0 }
    }

    /// Both halves at once, over one buffer.
    pub class Duplex : InStream, OutStream {
        pub name: String,

        /// A most-derived constructor: it is the one that places the shared `Stream`.
        ctor new(name: &str, buffer: Vec<u8>)
            : InStream(), OutStream(), virtual Stream { buffer, pos: 0 }
            { name: name.into() }

        override fn kind(&self) -> &'static str {
            "duplex"
        }
    }
}

impl Duplex {
    /// Reads a byte, advancing the shared cursor.
    fn read(&mut self) -> Option<u8> {
        let stream = self.as_stream_mut();
        let byte = stream.buffer.get(stream.pos).copied()?;
        stream.pos += 1;
        // `self.reads` reaches the `InStream` subobject through `Deref`.
        self.reads += 1;
        Some(byte)
    }

    /// Appends a byte to the same buffer the reader is walking.
    fn write(&mut self, byte: u8) {
        self.as_stream_mut().buffer.push(byte);
        self.as_out_stream_mut().writes += 1;
    }
}

fn main() {
    let mut duplex = Obj::<Duplex>::new(Duplex::new("socket", vec![b'h', b'i']));

    println!("== one shared base, reached two ways ==");
    // Both branches of the diamond resolve to the same address.
    let via_in: *const Stream = duplex.as_stream();
    let via_out: *const Stream = duplex.as_out_stream().as_stream();
    println!("  through InStream:  {via_in:p}");
    println!("  through OutStream: {via_out:p}");
    println!("  same subobject?    {}", via_in == via_out);

    println!("\n== so both halves share one cursor ==");
    println!("  remaining: {}", duplex.remaining());
    while let Some(byte) = duplex.read() {
        println!("  read {:?}, {} left", byte as char, duplex.remaining());
    }

    duplex.write(b'!');
    println!("  wrote '!', {} left to read", duplex.remaining());
    println!(
        "  {} reads, {} writes, buffer now {:?}",
        duplex.reads,
        duplex.as_out_stream().writes,
        String::from_utf8_lossy(&duplex.buffer),
    );

    println!("\n== through a base handle ==");
    let stream: Obj<Stream> = duplex.upcast();
    // `kind` is `Duplex`'s override, reached from a `Stream` handle across the shared base.
    println!("  kind      = {}", stream.kind());
    println!("  pos       = {}", stream.pos);
    println!("  class     = {}", stream.class().name);

    println!("\n== the base table ==");
    let meta = <Duplex as Class>::META;
    println!("  {} has {} entries:", meta.name, meta.bases.len());
    for base in meta.bases {
        println!("    offset {:>3}", base.data_offset);
    }
    println!(
        "  `Stream` appears once, at offset {} — past the whole `Duplex`, because it lives in\n  \
         the complete object rather than inside either branch.",
        meta.find_base(std::any::TypeId::of::<Stream>())
            .expect("present")
            .data_offset,
    );
}
