use hyper::Next;
use std::io::{ErrorKind, Result, Read, Write};

#[derive(Debug)]
pub struct Buffer {
    content: Vec<u8>,
    pos: usize,

    /// growable is either:
    ///   - false when reading a fixed buffer (Content-Length known in advance),
    ///     in which case it is only allocated once.
    ///   - true when using Transfer-Encoding: chunked, and the buffer grows dynamically
    growable: bool
}

const DEFAULT_BUF_SIZE: usize = 4 * 1024;

impl Buffer {
    /// Creates a new growable buffer
    pub fn new() -> Buffer {
        Buffer {
            content: Vec::new(),
            pos: 0,
            growable: true
        }
    }

    /// Creates a new fixed size buffer.
    pub fn new_fixed(capacity: usize) -> Buffer {
        debug!("creating fixed buffer with capacity {}", capacity);
        Buffer {
            content: vec![0; capacity],
            pos: 0,
            growable: false
        }
    }

    /// used when writing to check whether the buffer still has data
    pub fn is_empty(&self) -> bool { self.pos == self.len() }

    /// returns the length of this buffer's content
    pub fn len(&self) -> usize { self.content.len() }

    /// Read from the given reader into this buffer.
    ///
    /// Returns Ok(true) when done, Ok(false) otherwise, and Err if there is an error.
    pub fn read_from<R: Read>(&mut self, reader: &mut R) -> Result<bool> {
        loop {
            if self.growable {
                let mut len = self.len();
                if self.pos == len {
                    // if buffer is full, extend it
                    if len < DEFAULT_BUF_SIZE {
                        len = DEFAULT_BUF_SIZE;
                    } else {
                        len *= 2;
                    }

                    self.content.resize(len, 0);
                    debug!("buffer is full, grown to {}", self.len());
                }
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
                    self.pos += n;
                    if !self.growable && self.pos == self.len() {
                        return Ok(true);
                    }
                }
                Err(e) => {
                    return match e.kind() {
                        ErrorKind::WouldBlock => {
                            debug!("reading more would block");
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
            growable: true
        }
    }
}
