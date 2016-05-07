//! Edge is a Web framework that aims to be simple to use, with the most common things you need out of the box.
//! There are no plugins, the framework is not modular, but it is simple to use and easy to contribute to.
//!
//! The crate exports the things that you often need from dependencies, such as headers (from `hyper`),
//! cookies (from `cookie`) and JSON serialization (from `serde_json`).
//!
//! *Warning*: this is a very early version, and the API is not fully stable yet.
//!
//! ## Overview
//!
//! In Edge you must define an *application structure* that contains the state of your application.
//! You instantiate a container around this application, and associate GET/POST/... requests
//! with given URLs to methods of your application. The container handles the routing and
//! delegates calls to the appropriate methods.
//!
//! Note that the state cannot be mutated, as is usual in Rust (and enforced by the underlying HTTP server
//! this crate uses, a.k.a. Hyper). Any shared mutable variable must be wrapped in a `Mutex`.
//!
//! ## Why another Web framework in Rust?
//!
//! Because I wanted a simple Web framework with:
//!
//!   1. everything I needed out of the box, like cookies and forms and templating, without having to dig up third-party crates,
//!   1. the possibility to describe my application as a struct, so that callbacks could use a state (even if just for configuration).
//!
//! I hope you like this crate, if it misses something to fit your needs just open an issue or make a pull request!
//!
//! And please keep in mind that the framework is in a (very) early stage :-)
//!
//! ## Hello World
//!
//! ```no_run
//! extern crate edge;
//!
//! use edge::{Container, Request, Response, Status};
//! use edge::header::Server;
//! use std::io::Result;
//! use std::sync::Mutex;
//!
//! struct MyApp {
//!     version: &'static str,
//!     counter: Mutex<u32>
//! }
//!
//! impl MyApp {
//!     fn home(&self, _req: &mut Request, mut res: Response) -> Result<()> {
//!         let cnt = {
//!             let mut counter = self.counter.lock().unwrap();
//!             *counter += 1;
//!             *counter
//!         };
//!
//!         res.status(Status::Ok).content_type("text/plain");
//!         res.header(Server(format!("Edge version {}", self.version)));
//!         res.send(format!("Hello, world! {} visits", cnt))
//!     }
//! }
//!
//! fn main() {
//!     let app = MyApp { version: "0.1", counter: Mutex::new(0) };
//!     let mut cter = Container::new(app);
//!     cter.get("/", MyApp::home);
//!     cter.start("0.0.0.0:3000").unwrap();
//! }
//! ```

extern crate hyper;
extern crate url;
extern crate handlebars;
extern crate serde;
extern crate serde_json;

pub use hyper::header as header;
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

pub use serde_json::value as value;

use header::ContentLength;
use hyper::{Control, Decoder, Encoder, Method, Next, Get, Post, Head, Delete};
use hyper::method::Method::Put;
use hyper::net::HttpStream;
use hyper::server::{Handler, HandlerFactory, Server};
use hyper::server::{Request as HttpRequest, Response as HttpResponse};

use std::io::{Read, Result, Write};

use std::sync::Arc;

mod buffer;
mod router;
mod request;
mod response;

pub use request::Request;
pub use response::Response;

use buffer::Buffer;
use router::{Router, Callback};
use response::Resp;

/// Structure for an Edge application.
pub struct Edge<T: Send + Sync> {
    inner: Arc<T>,
    router: Arc<Router<T>>
}

impl<T: 'static + Send + Sync> Edge<T> {

    /// Creates an Edge application using the given inner structure.
    pub fn new(inner: T) -> Edge<T> {
        Edge {
            inner: Arc::new(inner),
            router: Arc::new(Router::new())
        }
    }

    /// Registers a callback for the given path for GE requests.
    pub fn get(&mut self, path: &str, callback: Callback<T>) {
        self.insert(Get, path, callback);
    }

    /// Registers a callback for the given path for POST requests.
    pub fn post(&mut self, path: &str, callback: Callback<T>) {
        self.insert(Post, path, callback);
    }

    /// Registers a callback for the given path for PUT requests.
    pub fn put(&mut self, path: &str, callback: Callback<T>) {
        self.insert(Put, path, callback);
    }

    /// Registers a callback for the given path for DELETE requests.
    pub fn delete(&mut self, path: &str, callback: Callback<T>) {
        self.insert(Delete, path, callback);
    }

    /// Registers a callback for the given path for HEAD requests.
    pub fn head(&mut self, path: &str, callback: Callback<T>) {
        self.insert(Head, path, callback);
    }

    /// Inserts the given callback for the given method and given route.
    pub fn insert(&mut self, method: Method, path: &str, callback: Callback<T>) {
        let router = Arc::get_mut(&mut self.router).unwrap();
        router.insert(method, path.into(), callback)
    }

    /// Starts a server.
    pub fn start(self, addr: &str) -> Result<()> {
        let server = Server::http(&addr.parse().unwrap()).unwrap();
        server.handle(self).unwrap();
        Ok(())
    }

}

pub struct EdgeHandler<T: Send + Sync> {
    router: Arc<Router<T>>,
    app: Arc<T>,

    request: Option<Request>,
    body: Option<Buffer>,
    resp: Option<Arc<Resp>>
}

impl<T: 'static + Send + Sync> HandlerFactory<HttpStream> for Edge<T> {
    type Output = EdgeHandler<T>;

    fn create(&mut self, control: Control) -> EdgeHandler<T> {
        EdgeHandler {
            router: self.router.clone(),
            app: self.inner.clone(),

            request: None,
            body: None,
            resp: Some(Arc::new(Resp::new(control)))
        }
    }
}

fn is_response_done(resp_opt: &mut Option<Arc<Resp>>) -> bool {
    if let Some(ref mut arc) = *resp_opt {
        return Arc::get_mut(arc).is_some();
    }
    false
}

impl<T: 'static + Send + Sync> EdgeHandler<T> {
    fn callback(&mut self) -> Next {
        let req = &mut self.request.as_mut().unwrap();

        if let Some(callback) = self.router.find_callback(req) {
            let res = response::new(&self.resp);
            callback(&self.app, req, res);
        } else {
            println!("route not found for path {:?}", req.path());
            let mut res = response::new(&self.resp);
            res.status(Status::NotFound);
            res.content_type("text/plain");
            res.send(format!("not found: {:?}", req.path()));
        }

        if is_response_done(&mut self.resp) {
            println!("response done, return Next::write after callback");
            Next::write()
        } else {
            // otherwise we ask the Response to notify us, and wait
            println!("response not done, return Next::wait after callback");
            response::set_notify(&self.resp);
            Next::wait()
        }
    }
}

/// Implements Handler for our EdgeHandler.
impl<T: 'static + Send + Sync> Handler<HttpStream> for EdgeHandler<T> {
    fn on_request(&mut self, req: HttpRequest) -> Next {
        println!("on_request");

        match request::new(req) {
            Ok(req) => {
                self.body = match *req.method() {
                    Put | Post => Some(match req.headers().get::<ContentLength>() {
                        Some(&ContentLength(len)) => Buffer::with_capacity(len as usize),
                        None => Buffer::new()
                    }),
                    _ => None
                };

                self.request = Some(req);
                if self.body.is_some() {
                    Next::read()
                } else {
                    self.callback()
                }
            },
            Err(error) => {
                let mut res = response::new(&self.resp);
                res.status(Status::BadRequest);
                res.content_type("text/plain");
                res.send(error.to_string());
                Next::write()
            }
        }
    }

    fn on_request_readable(&mut self, transport: &mut Decoder<HttpStream>) -> Next {
        println!("on_request_readable");

        // we can only get here if self.body = Some(...), or there is a bug
        {
            let body = self.body.as_mut().unwrap();
            if let Some(next) = body.read(transport) {
                return next;
            }
        }

        // move body to the request
        request::set_body(self.request.as_mut(), self.body.take());
        self.callback()
    }

    fn on_response(&mut self, res: &mut HttpResponse) -> Next {
        println!("on_response");

        // we got here from callback directly or Resp notified the Control
        let resp = Arc::try_unwrap(self.resp.take().unwrap()).unwrap();

        let (status, headers, body) = resp.deconstruct();
        res.set_status(status);
        *res.headers_mut() = headers;

        if body.is_empty() {
            Next::end()
        } else {
            self.body = Some(body);
            Next::write()
        }
    }

    fn on_response_writable(&mut self, transport: &mut Encoder<HttpStream>) -> Next {
        println!("on_response_writable");

        let body = self.body.as_mut().unwrap();
        if body.is_empty() {
            // done writing the buffer
            println!("done writing");
            Next::end()
        } else {
            // repeatedly write the body here with Next::write
            body.write(transport)
        }
    }
}
