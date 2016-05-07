//! Edge is a Web framework that aims to be simple to use and powerful, with the most common things
//! you need out of the box; we focus on integration rather than modularity. It supports both
//! synchronous and asynchronous style request handling, see below for examples.
//!
//! The crate exports the things that you often need from dependencies, such as headers (from `hyper`),
//! cookies (from `cookie`) and JSON serialization (from `serde_json`).
//!
//! *Warning*: this is an early version, and the API is not fully stable yet.
//!
//! ## Overview
//!
//! In Edge you must define an *application structure* that contains the state of your application.
//! You instantiate a container around this application, and associate GET/POST/... requests
//! with given URLs to methods of your application. The container handles the routing and
//! delegates calls to the appropriate methods.
//!
//! Note that the state cannot be mutated, as is usual in Rust (and enforced by the underlying HTTP server
//! this crate uses, a.k.a. Hyper). Use appropriate concurrent data structures if you need
//! shared mutable variables: locks, mutexes, channels, etc.
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
//! The most basic application: no state, a single page that prints Hello, world!
//!
//! ```no_run
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response};
//!
//! struct MyApp;
//! impl MyApp {
//!     fn hello(&self, _req: &mut Request, mut res: Response) {
//!         res.content_type("text/plain");
//!         res.send("Hello, world!")
//!     }
//! }
//!
//! fn main() {
//!     let mut cter = Edge::new(MyApp);
//!     cter.get("/", MyApp::hello);
//!     cter.start("0.0.0.0:3000").unwrap();
//! }
//! ```
//!
//! ## Asynchronous handling
//!
//! Under the hood, Edge uses the asynchronous version of Hyper. This means that to get the maximum
//! performance, you should avoid waiting in a handler, so that other requests
//! can be served as soon as possible. In that example, the handler waits in a separate thread before sending
//! the response.
//!
//! ```no_run
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response};
//! use std::thread;
//! use std::time::Duration;
//!
//! struct MyApp;
//! impl MyApp {
//!     fn hello(&self, _req: &mut Request, mut res: Response) {
//!         thread::spawn(move || {
//!             println!("waiting 1 second");
//!             thread::sleep(Duration::from_secs(1));
//!
//!             res.content_type("text/plain");
//!             res.send("Hello, world!")
//!         });
//!
//!         // the handler returns immediately without waiting for the thread
//!     }
//! }
//!
//! fn main() {
//!     let mut cter = Edge::new(MyApp);
//!     cter.get("/", MyApp::hello);
//!     cter.start("0.0.0.0:3000").unwrap();
//! }
//! ```
//!
//! ## Templating
//!
//! Here our application has a version, still a single handler except this time
//! it accepts any page name, and renders a Handlebars template.  We're also
//! setting a custom Server header.
//!
//! ```no_run
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response, Status};
//! use edge::header::Server;
//! use std::collections::BTreeMap;
//!
//! struct MyApp {
//!     version: &'static str
//! }
//!
//! impl MyApp {
//!     fn page_handler(&self, req: &mut Request, mut res: Response) {
//!         let mut data = BTreeMap::new();
//!         data.insert("title", req.param("page").unwrap());
//!         data.insert("version", self.version);
//!
//!         res.content_type("text/html").header(Server(format!("Edge version {}", self.version)));
//!         res.render("views/page.hbs", data)
//!     }
//! }
//!
//! fn main() {
//!     let app = MyApp { version: "0.1" };
//!     let mut cter = Edge::new(app);
//!     cter.get("/:page", MyApp::page_handler);
//!     cter.start("0.0.0.0:3000").unwrap();
//! }
//! ```
//!
//! ## Using a shared mutable counter
//!
//! In this example, we use an atomic integer to track a counter. This shows a very basic
//! kind of shared state for a handler. In practice, it's best to avoid using blocking
//! mechanisms (locks, mutexes) in a handler directly. Prefer non-blocking calls,
//! like channels' try_recv, or move blocking code in a separate thread,
//! see the example for asynchronous handling above.
//!
//! ```no_run
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response, Status};
//! use std::sync::atomic::{AtomicUsize, Ordering};
//!
//! struct MyApp {
//!     counter: AtomicUsize
//! }
//!
//! impl MyApp {
//!     fn home(&self, _req: &mut Request, mut res: Response) {
//!         let visits = self.counter.load(Ordering::Relaxed);
//!         self.counter.store(visits + 1, Ordering::Relaxed);
//!
//!         res.status(Status::Ok).content_type("text/plain");
//!         res.send(format!("Hello, world! {} visits", visits))
//!     }
//! }
//!
//! fn main() {
//!     let app = MyApp { counter: AtomicUsize::new(0) };
//!     let mut cter = Edge::new(app);
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
