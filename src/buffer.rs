use hyper::Next;
use std::io::{ErrorKind, Read, Write};

#[derive(Debug)]
pub struct Buffer {
    content: Vec<u8>,
    pos: usize,

    /// growable is either None when reading a fixed buffer (Content-Length known in advance)
    /// or it is Some(size) where size is the current write size by which the buffer should be grown
    /// every time it is full
    growable: Option<usize>
}

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            content: Vec::new(),
            pos: 0,
            growable: Some(16)
        }
    }

    pub fn with_capacity(capacity: usize) -> Buffer {
        println!("creating buffer with capacity {}", capacity);
        Buffer {
            content: vec![0; capacity],
            pos: 0,
            growable: None
        }
    }

    /// used when writing to check whether the buffer still has data
    pub fn is_empty(&self) -> bool { self.pos == self.len() }

    /// used when reading to check whether the buffer can still hold data
    pub fn is_full(&self) -> bool { self.pos == self.len() }

    /// returns the length of this buffer's content
    pub fn len(&self) -> usize { self.content.len() }

    /// read from the given reader into this buffer
    pub fn read<R: Read>(&mut self, reader: &mut R) -> Option<Next> {
        if let Some(mut write_size) = self.growable {
            // if buffer is growable, check whether it is full
            if self.is_full() {
                let len = self.len();
                // if buffer is full, extend it
                // reused Read::read_to_end algorithm
                if write_size < DEFAULT_BUF_SIZE {
                    write_size *= 2;
                    self.growable = Some(write_size);
                }
                self.content.resize(len + write_size, 0);
            }
        }

        match reader.read(&mut self.content[self.pos..]) {
            Ok(0) => {
                // note: EOF is supposed to happen only when reading a growable buffer
                if self.growable.is_some() {
                    // we truncate the buffer so it has the proper size
                    self.content.truncate(self.pos);
                }
                None
            }
            Ok(n) => {
                self.pos += n;
                if self.growable.is_none() && self.is_full() {
                    // fixed size full buffer, nothing to read anymore
                    None
                } else {
                    // fixed size buffer not full, or growable buffer
                    Some(Next::read())
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock => Some(Next::read()),
                _ => {
                    println!("read error {:?}", e);
                    Some(Next::end())
                }
            }
        }
    }

    pub fn send<D: Into<Vec<u8>>>(&mut self, content: D) {
        self.content = content.into();
    }

    /// writes from this buffer into the given writer
    pub fn write<W: Write>(&mut self, writer: &mut W) -> Next {
        match writer.write(&self.content[self.pos..]) {
            Ok(0) => panic!("wrote 0 bytes"),
            Ok(n) => {
                println!("wrote {} bytes", n);
                self.pos += n;
                if self.is_empty() {
                    // done reading
                    Next::end()
                } else {
                    Next::write()
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock => Next::write(),
                _ => {
                    println!("write error {:?}", e);
                    Next::end()
                }
            }
        }
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        &self.content
    }
}

impl<'a> From<&'a [u8]> for Buffer {
    fn from(content: &[u8]) -> Buffer {
        Buffer {
            content: content.to_vec(),
            pos: 0,
            growable: Some(16)
        }
    }
}