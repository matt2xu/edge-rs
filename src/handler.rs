use handlebars::Handlebars;

use hyper::{Control, Decoder, Encoder, Next};
use hyper::HttpVersion::{Http09, Http10, Http11};

use hyper::error::Error as HyperError;
use hyper::header::{ContentLength, Encoding, TransferEncoding};
use hyper::method::Method::{Connect, Delete, Get, Head, Trace};
use hyper::net::HttpStream;
use hyper::server::{Handler, Request as HttpRequest, Response as HttpResponse};
use hyper::status::StatusCode as Status;

use buffer::Buffer;
use request::{self, Request};
use response::ResponseHolder;
use router::Router;

use std::sync::Arc;

/// a handler
pub struct EdgeHandler<'a, T: 'a> {
    app: T,
    router: &'a Router<T>,
    request: Option<Request>,
    buffer: Option<Buffer>,
    holder: ResponseHolder
}

impl<'a, T> EdgeHandler<'a, T> {
    pub fn new(app: T, router: &'a Router<T>, handlebars: Arc<Handlebars>, control: Control) -> EdgeHandler<T> {
        EdgeHandler {
            app: app,
            router: router,
            request: None,
            buffer: None,
            holder: ResponseHolder::new(handlebars, control)
        }
    }

    fn callback(&mut self) -> Next {
        let req = &mut self.request.as_mut().unwrap();

        if let Some(callback) = self.router.find_callback(req) {
            callback(&self.app, req, self.holder.new_response());
        } else {
            warn!("route not found for path {:?}", req.path());
            let mut res = self.holder.new_response();
            res.status(Status::NotFound);
            res.content_type("text/plain");
            res.send(format!("not found: {:?}", req.path()));
        }

        if self.holder.can_write() {
            debug!("response done, return Next::write after callback");
            Next::write()
        } else {
            // otherwise we ask the Response to notify us, and wait
            debug!("response not done, return Next::wait after callback");
            Next::wait()
        }
    }

    fn bad_request(&mut self, message: &str) -> Next {
        error!("Bad Request: {}", message);

        let mut res = self.holder.new_response();
        res.status(Status::BadRequest).content_type("text/plain; charset=UTF-8");
        res.send(message);
        Next::write()
    }

}

/// Implements Handler for our EdgeHandler.
impl<'a, T> Handler<HttpStream> for EdgeHandler<'a, T> {
    fn on_request(&mut self, req: HttpRequest) -> Next {
        debug!("on_request");

        match request::new(&self.router.base_url, req) {
            Ok(req) => {
                let result = check_request(&req, &mut self.buffer);
                self.request = Some(req);

                match result {
                    Err(msg) => self.bad_request(msg),
                    Ok(false) => self.callback(),
                    Ok(true) => Next::read()
                }
            }
            Err(error) => {
                self.bad_request(&error.to_string())
            }
        }
    }

    fn on_request_readable(&mut self, transport: &mut Decoder<HttpStream>) -> Next {
        debug!("on_request_readable");

        // we can only get here if self.buffer = Some(...), or there is a bug
        {
            let body = self.buffer.as_mut().unwrap();
            if let Ok(keep_reading) = body.read_from(transport) {
                if keep_reading {
                    return Next::read();
                }
            }
        }

        // move body to the request
        request::set_body(self.request.as_mut(), self.buffer.take());
        self.callback()
    }

    fn on_response(&mut self, res: &mut HttpResponse) -> Next {
        debug!("on_response");

        // we got here from callback directly or Resp notified the Control
        let status = self.holder.get_status();

        // set status and headers
        res.set_status(status);
        self.holder.set_headers(res.headers_mut());

        // 3.3.2 Content-Length
        // http://httpwg.org/specs/rfc7230.html#header.content-length
        //
        // A server MUST NOT send a Content-Length header field in any response
        // with a status code of 1xx (Informational) or 204 (No Content).
        //
        // A server MAY send a Content-Length header field in a response to a HEAD request
        // A server MAY send a Content-Length header field in a 304 (Not Modified) response
        if status.is_informational() ||
            status == Status::NoContent || status == Status::NotModified ||
            *self.request.as_ref().unwrap().method() == Head {
            // we remove any ContentLength header in those cases
            // even in 304 and response to HEAD
            // because we cannot guarantee that the length is the same
            res.headers_mut().remove::<ContentLength>();
            return Next::end();
        }

        if !self.holder.body().is_empty() {
            debug!("has body");
            Next::write()
        } else if self.holder.is_streaming() {
            debug!("streaming mode, waiting");
            Next::wait()
        } else {
            debug!("has no body, ending");
            Next::end()
        }
    }

    fn on_response_writable(&mut self, transport: &mut Encoder<HttpStream>) -> Next {
        debug!("on_response_writable");

        if self.holder.is_streaming() {
            if self.buffer.is_none() {
                self.buffer = self.holder.pop();
            }

            if self.buffer.is_none() {
                // no buffer available yet
                Next::wait()
            } else {
                let empty = self.buffer.as_mut().unwrap().is_empty();
                if empty {
                    // an empty body means no more data to write
                    debug!("done writing");
                    Next::end()
                } else {
                    let result = self.buffer.as_mut().unwrap().write_to(transport);
                    if let Ok(keep_writing) = result {
                        if keep_writing {
                            Next::write()
                        } else {
                            // done writing this buffer, try to get another one
                            self.buffer = None;
                            Next::write()
                        }
                    } else {
                        Next::remove()
                    }
                }
            }
        } else {
            if let Ok(keep_writing) = self.holder.body().write_to(transport) {
                if keep_writing {
                    Next::write()
                } else {
                    Next::end()
                }
            } else {
                Next::remove()
            }
        }
    }

    fn on_error(&mut self, err: HyperError) -> Next {
        debug!("on_error {:?}", err);
        Next::remove()
    }

    fn on_remove(self, _transport: HttpStream) {
        debug!("on_remove");
    }
}

fn check_request(req: &Request, buffer: &mut Option<Buffer>) -> ::std::result::Result<bool, &'static str> {
    let headers = req.headers();
    let http1x = { let version = req.version(); *version == Http09 || *version == Http10 || *version == Http11 };

    // RFC 7230 Hypertext Transfer Protocol (HTTP/1.1): Message Syntax and Routing
    // 3.3.3 Message Body Length
    // http://httpwg.org/specs/rfc7230.html#message.body.length
    //
    let len =
        if let Some(&TransferEncoding(ref codings)) = headers.get() {
            if codings.last() != Some(&Encoding::Chunked) {
                // 3. If a Transfer-Encoding header field is present in a request and
                // the chunked transfer coding is not the final encoding,
                // the message body length cannot be determined reliably;
                // the server MUST respond with the 400 (Bad Request) status code
                // and then close the connection.
                return Err("Last encoding of Transfer-Encoding must be chunked")
            }

            // Transfer-Encoding is correct, we have a payload
            None
        } else if let Some(&ContentLength(len)) = headers.get() {
            // 5. If a valid Content-Length header field is present without Transfer-Encoding,
            // its decimal value defines the expected message body length in octets.
            if len == 0 {
                info!("Request with empty payload Content-Length: 0");
                return Ok(false);
            }

            Some(len as usize)
        } else if headers.has::<ContentLength>() {
            // 4. If a message is received without Transfer-Encoding
            // and with either multiple Content-Length header fields having
            // differing field-values or a single Content-Length header field
            // having an invalid value, then the message framing is invalid
            // and the recipient MUST treat it as an unrecoverable error.
            // If this is a request message, the server MUST respond with
            // a 400 (Bad Request) status code and then close the connection.
            return Err("Invalid Content-Length header");
        } else if http1x {
            // If this is a request message and none of the above are true,
            // then the message body length is zero (no message body is present).
            return Ok(false);
        } else {
            // in HTTP/2 a request message can have a body even if
            // no Transfer-Encoding or Content-Length headers are present
            None
        };

    // RFC 7231 Hypertext Transfer Protocol (HTTP/1.1): Semantics and Content
    // 4.3 Method Definitions
    // http://httpwg.org/specs/rfc7231.html#method.definitions
    //
    // A payload within a GET/HEAD/DELETE/CONNECT request message has no defined
    // semantics; sending a payload body on a GET/HEAD/DELETE/CONNECT request
    // might cause some existing implementations to reject the request.
    let method = req.method();
    if *method == Get || *method == Head || *method == Delete || *method == Connect {
        // payload in these methods has no defined semantics
        if http1x {
            // warns only for HTTP/1.x-compatible
            // because we cannot know for sure whether there is a payload in HTTP/2
            warn!("Ignoring payload for {} request", *method);
        }

        Ok(false)
    } else if *method == Trace {
        Err("A client MUST NOT send a message body in a TRACE request.")
    } else {
        // payload is allowed
        // if Content-Length is known create buffer with fixed size, otherwise allocate growable buffer
        *buffer = Some(len.map_or(Buffer::new(), |len| Buffer::new_fixed(len)));
        Ok(true)
    }
}
