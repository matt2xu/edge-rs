use hyper::Next;
use std::io::{ErrorKind, Result, Read, Write};

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
        debug!("creating buffer with capacity {}", capacity);
        Buffer {
            content: vec![0; capacity],
            pos: 0,
            growable: None
        }
    }

    /// used when writing to check whether the buffer still has data
    pub fn is_empty(&self) -> bool { self.pos == self.len() }

    /// returns the length of this buffer's content
    pub fn len(&self) -> usize { self.content.len() }

    /// Read from the given reader into this buffer.
    ///
    /// Returns Ok(true) when done, Ok(false) otherwise, and Err if there is an error.
    pub fn read<R: Read>(&mut self, reader: &mut R) -> Result<bool> {
        match self.growable {
            None => self.read_fixed(reader),
            Some(_) => self.read_growable(reader)
        }
    }

    fn read_fixed<R: Read>(&mut self, reader: &mut R) -> Result<bool> {
        match reader.read(&mut self.content[self.pos..]) {
            Ok(n) => {
                self.pos += n;
                Ok(self.pos == self.len())
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock => Ok(false),
                _ => Err(e)
            }
        }
    }

    fn read_growable<R: Read>(&mut self, reader: &mut R) -> Result<bool> {
        let mut write_size = self.growable.unwrap_or(16);
        loop {
            let len = self.len();
            if self.pos == len {
                // if buffer is full, extend it
                // reused Read::read_to_end algorithm
                if write_size < DEFAULT_BUF_SIZE {
                    write_size *= 2;
                }
                self.content.resize(len + write_size, 0);
                debug!("buffer is full, growing to {}", self.len());
            }

            match reader.read(&mut self.content[self.pos..]) {
                Ok(0) => {
                    // EOF, we truncate the buffer so it has the proper size
                    debug!("EOF, content is {} bytes", self.pos);
                    self.content.truncate(self.pos);
                    return Ok(true);
                }
                Ok(n) => {
                    // got n bytes, loop to determine if we need to read again
                    debug!("read {} bytes from transport", n);
                    self.pos += n
                }
                Err(e) => {
                    return match e.kind() {
                        ErrorKind::WouldBlock => {
                            debug!("reading more would block");
                            self.growable = Some(write_size);
                            Ok(false)
                        },
                        _ => {
                            error!("error while reading: {}", e);
                            Err(e)
                        }
                    };
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

impl From<Vec<u8>> for Buffer {
    fn from(content: Vec<u8>) -> Buffer {
        Buffer {
            content: content,
            pos: 0,
            growable: Some(16)
        }
    }
}
