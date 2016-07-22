use handlebars::Handlebars;

use hyper::{Control, Decoder, Encoder, Next};
use hyper::HttpVersion::{Http09, Http10, Http11};

use hyper::error::Error as HyperError;
use hyper::header::{ContentLength, ContentType, Encoding, TransferEncoding};
use hyper::method::Method::{Connect, Delete, Get, Head, Trace};
use hyper::net::HttpStream;
use hyper::server::{Handler, Request as HttpRequest, Response as HttpResponse};
use hyper::status::StatusCode as Status;

use scoped_pool::Scope;

use serde_json::value as json;

use url::Url;

use buffer::Buffer;
use request::{self, Request};
use response::{self, Response, Result, Action};
use router::{Callback, RouterAny};

use crossbeam::sync::chase_lev::{deque, Steal, Stealer, Worker};

use std::any::Any;
use std::io::{self, Write};

enum Reply {
    Headers(Response),
    Body(Buffer)
}

enum Body {
    Empty,
    Some(Buffer),
    Streaming(Box<Fn(&mut Any, &mut Write)>)
}

struct Stream {
    worker: Worker<Reply>,
    control: Control
}

fn notify(control: &Control) {
    if let Err(e) = control.ready(Next::write()) {
        error!("could not notify handler: {}", e);
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.worker.push(Reply::Body(buf.to_vec().into()));
        notify(&self.control);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.worker.push(Reply::Body(vec![].into()));
        notify(&self.control);
    }
}

/// a handler that lasts only the time of a request
/// scope outlives handler
pub struct EdgeHandler<'handler, 'scope: 'handler> {
    scope: &'handler Scope<'scope>,
    base_url: &'handler Url,
    routers: &'scope [RouterAny],
    request: Option<Request>,
    is_head_request: bool,
    buffer: Option<Buffer>,

    handlebars: &'scope Handlebars,
    control: Control,
    stealer: Option<Stealer<Reply>>,
    streaming: bool
}

impl<'handler, 'scope> EdgeHandler<'handler, 'scope> {
    pub fn new(scope: &'handler Scope<'scope>, base_url: &'handler Url, routers: &'scope [RouterAny], handlebars: &'scope Handlebars, control: Control) -> EdgeHandler<'handler, 'scope> {
        EdgeHandler {
            scope: scope,
            base_url: base_url,
            routers: routers,
            request: None,
            is_head_request: false,
            buffer: None,

            handlebars: handlebars,
            control: control,
            stealer: None,
            streaming: false
        }
    }

    fn callback(&mut self) -> Next {
        let (mut worker, stealer) = deque();
        self.stealer = Some(stealer);

        let mut req = self.request.take().unwrap();

        let result = self.routers.iter().filter_map(|router|
            if let Some(callback) = router.find_callback(&mut req) {
                Some((router, callback))
            } else {
                None
            }
        ).next();

        if let Some((router, callback)) = result {
            // add job to scoped pool
            let ctrl = self.control.clone();
            let handlebars = self.handlebars;

            self.scope.execute(move || {
                let mut response = Response::new();
                let mut boxed_app = router.new_instance();
                let app = boxed_app.as_mut();
                let result =
                    match *callback {
                        Callback::Instance(ref f) => {
                            router.run_middleware(app, &mut req, &mut response);
                            f(app, &req, &mut response)
                        }
                        Callback::Static(ref f) => f(&req, &mut response)
                    };

                match process_handle_result(&mut response, result, handlebars) {
                    Body::Empty => {
                        worker.push(Reply::Headers(response));
                        notify(&ctrl);
                    }
                    Body::Some(body) => {
                        response.len(body.len() as u64);
                        worker.push(Reply::Headers(response));
                        worker.push(Reply::Body(body));
                        notify(&ctrl);
                    }
                    Body::Streaming(closure) => {
                        worker.push(Reply::Headers(response));
                        notify(&ctrl);

                        let mut stream = Stream {
                            worker: worker,
                            control: ctrl
                        };
                        closure(app, &mut stream);
                    }
                }
            });

            // and wait for it to notify us
            Next::wait()
        } else {
            //warn!("route not found for path {:?}", req.path())
            let mut response = Response::new();
            response.status(Status::NotFound).content_type("text/plain");
            worker.push(Reply::Headers(response));
            worker.push(Reply::Body(format!("not found: {:?}", req.path()).into_bytes().into()));
            Next::write()
        }
    }

    fn bad_request(&mut self, message: &str) -> Next {
        let (mut worker, stealer) = deque();
        self.stealer = Some(stealer);

        error!("Bad Request: {}", message);
        let mut response = Response::new();
        response.status(Status::BadRequest).content_type("text/plain; charset=UTF-8");
        worker.push(Reply::Headers(response));
        worker.push(Reply::Body(message.to_string().into_bytes().into()));
        Next::write()
    }

}

/// Matches the result to update the response and produce a body.
///
/// If the result is Ok, converts the value into a HandleResult, and calls
/// end/send/render/redirect depending on the type of result.
/// Otherwise, if the result is Err, sets the status with the error message as content (if specified).
/// as the body.
fn process_handle_result(response: &mut Response, result: Result, handlebars: &Handlebars) -> Body {
    match result {
        Ok(handler) => {
            match handler.into() {
                Action::End(status) => {
                    if let Some(status) = status {
                        response.status(status);
                    }
                    Body::Empty
                }
                Action::Redirect(status, url) => {
                    response.status(status);
                    response.location(url);
                    Body::Empty
                }
                Action::Render(name, json) => {
                    let buffer = render(response, handlebars, &name, &json);
                    Body::Some(buffer)
                }
                Action::Send(body) => {
                    Body::Some(body.into())
                }
                Action::SendFile(filename) => {
                    if let Some(body) = response::send_file(response, filename).map(|vec| vec.into()) {
                        Body::Some(body)
                    } else {
                        Body::Empty
                    }
                }
                Action::Stream(closure) => {
                    response::set_streaming(response);
                    Body::Streaming(closure)
                }
            }
        }
        Err(error) => {
            match error.message {
                None => {
                    response.status(error.status);
                    Body::Empty
                }
                Some(message) => {
                    response.status(error.status);
                    response.content_type("text/plain");
                    Body::Some((&*message).as_bytes().to_vec().into())
                }
            }
        }
    }
}

/// Renders the template with the given name using the given data.
///
/// If no Content-Type header is set, the content type is set to `text/html`.
fn render(response: &mut Response, handlebars: &Handlebars, name: &str, json: &json::Value) -> Buffer {
    if !response.headers.has::<ContentType>() {
        response.header(ContentType::html());
    }

    let result = handlebars.render(name, json);
    result.unwrap().into_bytes().into()
}

/// Implements Handler for our EdgeHandler.
impl<'handler, 'scope> Handler<HttpStream> for EdgeHandler<'handler, 'scope> {
    fn on_request(&mut self, req: HttpRequest) -> Next {
        debug!("on_request");

        match request::new(&self.base_url, req) {
            Ok(req) => {
                let result = check_request(&req, &mut self.buffer);
                self.is_head_request = *req.method() == Head;
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
        loop {
            match self.stealer.as_ref().unwrap().steal() {
                Steal::Data(reply) => {
                    match reply {
                        Reply::Headers(response) => {
                            self.streaming = response::is_streaming(&response);
                            let status = response.status;

                            // set status and headers
                            res.set_status(status);
                            *res.headers_mut() = response.headers;

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
                                self.is_head_request {
                                // we remove any ContentLength header in those cases
                                // even in 304 and response to HEAD
                                // because we cannot guarantee that the length is the same
                                res.headers_mut().remove::<ContentLength>();
                                return Next::end();
                            }
                        }
                        Reply::Body(body) => {
                            debug!("has body");
                            self.buffer = Some(body);
                            return Next::write();
                        }
                    }
                }
                Steal::Empty => {
                    if self.streaming {
                        debug!("streaming mode, waiting");
                        return Next::wait();
                    } else {
                        debug!("has no body, ending");
                        return Next::end();
                    }
                }
                Steal::Abort => panic!("abort")
            }
        }
    }

    fn on_response_writable(&mut self, transport: &mut Encoder<HttpStream>) -> Next {
        debug!("on_response_writable");

        if self.streaming {
            if self.buffer.is_none() {
                self.buffer = match self.stealer.as_ref().unwrap().steal() {
                    Steal::Data(Reply::Body(body)) => Some(body),
                    Steal::Empty => None,
                    _ => panic!("unexpected")
                };
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
            let body = self.buffer.as_mut().unwrap();
            if let Ok(keep_writing) = body.write_to(transport) {
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
