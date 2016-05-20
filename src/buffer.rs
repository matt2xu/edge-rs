use std::io::{Error, ErrorKind, Result, Read, Write};

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

    /// Updates the capacity of this buffer.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.content.resize(capacity, 0);
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
                    if self.growable {
                        // EOF, we truncate the buffer so it has the proper size
                        debug!("EOF, content is {} bytes", self.pos);
                        self.content.truncate(self.pos);
                    } else {
                        // RFC 7230 Hypertext Transfer Protocol (HTTP/1.1): Message Syntax and Routing
                        // 3.3.3 Message Body Length
                        // http://httpwg.org/specs/rfc7230.html#message.body.length
                        //
                        // 5. If a valid Content-Length header field is present without Transfer-Encoding,
                        // its decimal value defines the expected message body length in octets.
                        // If the sender closes the connection or the recipient times out before the
                        // indicated number of octets are received, the recipient MUST consider the message
                        // to be incomplete and close the connection.
                        if self.pos != self.len() {
                            let message = format!("Incomplete message: expected {} bytes, got {}", self.len(), self.pos);
                            error!("error while reading: {}", message);
                            return Err(Error::new(ErrorKind::UnexpectedEof, message));
                        }
                    }

                    return Ok(false);
                }
                Ok(n) => {
                    // got n bytes, loop to determine if we need to read again
                    debug!("read {} bytes from transport", n);
                    self.pos += n;
                    if !self.growable && self.pos == self.len() {
                        return Ok(false);
                    }
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
                Ok(0) => {
                    return Err(Error::new(ErrorKind::WriteZero, "could not write to the transport"));
                }
                Ok(n) => {
                    debug!("wrote {} bytes", n);
                    self.pos += n;
                    if self.pos == self.len() {
                        // done writing
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
            pos: 0,
            growable: true
        }
    }
}
