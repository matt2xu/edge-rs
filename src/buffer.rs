use std::io::{ErrorKind, Result, Read, Write};

#[derive(Debug)]
pub struct Buffer {
    content: Vec<u8>,
    pos: usize
}

const DEFAULT_BUF_SIZE: usize = 4 * 1024;

impl Buffer {
    /// Creates a new buffer
    pub fn new() -> Buffer {
        Buffer {
            content: Vec::new(),
            pos: 0
        }
    }

    /// Updates the capacity of this buffer.
    pub fn set_capacity(&mut self, capacity: usize) {
        // increases the capacity to avoid an unnecessary allocation
        // really + 1 would be enough
        // but it feels better to align to the next multiple of 8 instead :-)
        self.content.resize(((capacity >> 3) + 1) << 3, 0);
    }

    /// used when writing to check whether the buffer still has data
    pub fn is_empty(&self) -> bool { self.pos == self.len() }

    /// returns the length of this buffer's content
    pub fn len(&self) -> usize { self.content.len() }

    /// Read from the given reader into this buffer.
    ///
    /// Returns Ok(true) when the handler needs to read from the transport,
    /// Ok(false) when done, and Err if there is an error.
    pub fn read_from<R: Read>(&mut self, reader: &mut R) -> Result<bool> {
        loop {
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

            match reader.read(&mut self.content[self.pos..]) {
                Ok(0) => {
                    // EOF, we truncate the buffer so it has the proper size
                    debug!("EOF, content is {} bytes", self.pos);
                    self.content.truncate(self.pos);
                    return Ok(false);
                }
                Ok(n) => {
                    // got n bytes, loop to determine if we need to read again
                    debug!("read {} bytes from transport", n);
                    self.pos += n;
                }
                Err(e) => {
                    return match e.kind() {
                        ErrorKind::WouldBlock => {
                            debug!("reading more would block");
                            Ok(true)
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

    /// Takes the contents of this buffer out of it, and resets the current read/write position.
    pub fn take(&mut self) -> Vec<u8> {
        self.pos = 0;
        ::std::mem::replace(&mut self.content, Vec::new())
    }

    /// Writes from this buffer into the given writer
    pub fn write_to<W: Write>(&mut self, writer: &mut W) -> Result<bool> {
        if self.pos == self.len() {
            debug!("EOF, wrote a total of {} bytes", self.pos);
            return Ok(false);
        }

        loop {
            match writer.write(&self.content[self.pos..]) {
                Ok(0) => panic!("wrote 0 bytes"),
                Ok(n) => {
                    debug!("wrote {} bytes", n);
                    self.pos += n;
                    if self.pos == self.len() {
                        // done reading
                        return Ok(false);
                    }
                }
                Err(e) => {
                    return match e.kind() {
                        ErrorKind::WouldBlock => {
                            debug!("writing more would block");
                            Ok(true)
                        }
                        _ => {
                            error!("error while writing: {}", e);
                            Err(e)
                        }
                    };
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
            pos: 0
        }
    }
}

impl Into<Vec<u8>> for Buffer {
    fn into(self) -> Vec<u8> {
        self.content
    }
}
