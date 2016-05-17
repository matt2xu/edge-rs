//! Edge is a Web framework that is simple to use, with the most common things
//! you need out of the box, and flexible, supporting both synchronous and asynchronous
//! request handling styles; see below for examples.
//!
//! The crate exports the things that you often need from dependencies, such as headers (from `hyper`),
//! cookies (from `cookie`) and JSON serialization (from `serde_json`).
//!
//! Please note that this is an early version, and the API is likely to evolve.
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
//! We focus on integration rather than modularity.
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
extern crate deque;

#[macro_use]
extern crate log;

pub use hyper::header as header;
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

pub use serde_json::value as value;

use header::ContentLength;

use hyper::{Client as HttpClient, Decoder, Encoder, Method, Next, Get, Post, Head, Delete};
use hyper::client::{Request as ClientRequest, Response as ClientResponse};
use hyper::method::Method::Put;
use hyper::net::HttpStream;
use hyper::server::{Handler, Server, Request as HttpRequest, Response as HttpResponse};

use std::io::{Read, Result, Write};
use std::net::SocketAddr;
use std::sync::Arc;

mod buffer;
mod router;
mod request;
mod response;

pub use request::Request;
pub use response::Response;
pub use router::Callback;

use buffer::Buffer;
use response::ResponseHolder;
use router::Router;

/// Structure for an Edge application.
pub struct Edge<T> {
    addr: SocketAddr,
    inner: Arc<T>,
    router: Arc<Router<T>>
}

impl<T> Edge<T> {

    /// Creates an Edge application using the given inner structure.
    pub fn new(addr: &str, inner: T) -> Edge<T> {
        Edge {
            addr: addr.parse().unwrap(),
            inner: Arc::new(inner),
            router: Arc::new(Router::new(addr))
        }
    }

    /// Registers a callback for the given path for GET requests.
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

    /// Runs the server and never returns.
    ///
    /// This will block the current thread.
    pub fn start(self) -> Result<()> {
        let server = Server::http(&self.addr).unwrap();
        let (listening, server) = server.handle(move |control| {
            debug!("creating new edge handler");

            EdgeHandler {
                router: self.router.clone(),
                app: self.inner.clone(),

                request: None,
                body: None,
                holder: ResponseHolder::new(control)
            }
        }).unwrap();

        info!("Listening on http://{}", listening);
        server.run();
        Ok(())
    }

}

pub struct EdgeHandler<T> {
    router: Arc<Router<T>>,
    app: Arc<T>,

    request: Option<Request>,
    body: Option<Buffer>,
    holder: ResponseHolder
}

impl<T> Drop for EdgeHandler<T> {
    fn drop(&mut self) {
        debug!("dropping edge handler");
    }
}

impl<T> EdgeHandler<T> {
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
}

/// Implements Handler for our EdgeHandler.
impl<T> Handler<HttpStream> for EdgeHandler<T> {
    fn on_request(&mut self, req: HttpRequest) -> Next {
        debug!("on_request");

        match request::new(&self.router.base_url, req) {
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
                let mut res = self.holder.new_response();
                res.status(Status::BadRequest);
                res.content_type("text/plain");
                res.send(error.to_string());
                Next::write()
            }
        }
    }

    fn on_request_readable(&mut self, transport: &mut Decoder<HttpStream>) -> Next {
        debug!("on_request_readable");

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
        debug!("on_response");

        // we got here from callback directly or Resp notified the Control
        res.set_status(self.holder.get_status());
        self.holder.set_headers(res.headers_mut());

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
            if self.body.is_none() {
                self.body = self.holder.pop();
            }

            if let Some(ref mut body) = self.body {
                if body.is_empty() {
                    // done writing the buffer
                    debug!("done writing");
                    return Next::end();
                } else {
                    // repeatedly write the body here with Next::write
                    let _ = body.write(transport);
                    if !body.is_empty() {
                        return Next::write();
                    }
                }
            } else {
                return Next::wait();
            }

            self.body = None;
            Next::write()
        } else {
            let body = self.holder.body();
            if body.is_empty() {
                // done writing the buffer
                debug!("done writing");
                Next::end()
            } else {
                // repeatedly write the body here with Next::write
                body.write(transport)
            }
        }
    }

    fn on_error(&mut self, err: hyper::error::Error) -> Next {
        debug!("on_error {:?}", err);
        Next::remove()
    }

    fn on_remove(self, _transport: HttpStream) {
        debug!("on_remove");
    }
}

pub struct Client {
    inner: HttpClient<ClientHandler>
}

impl Client {
    pub fn new() -> Client {
        Client {
            inner: HttpClient::new().unwrap()
        }
    }

    pub fn request<'a, F: 'static + FnMut(Vec<u8>) + Send, I: AsRef<str>>(&mut self, url: I, callback: F) {
        let _ = self.inner.request(url.as_ref().parse().unwrap(), ClientHandler {
            callback: Box::new(callback)
        });
    }
}

use std::io;
use std::boxed::Box;

struct ClientHandler {
    callback: Box<FnMut(Vec<u8>) + Send>
}

impl hyper::client::Handler<HttpStream> for ClientHandler {

    fn on_request(&mut self, _req: &mut ClientRequest) -> Next {
        Next::read()
    }

    fn on_request_writable(&mut self, _encoder: &mut Encoder<HttpStream>) -> Next {
        Next::read()
    }

    fn on_response(&mut self, res: ClientResponse) -> Next {
        println!("Response: {}", res.status());
        println!("Headers:\n{}", res.headers());
        Next::read()
    }

    fn on_response_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
        let mut buf = vec![0; 65536];
        match decoder.read(&mut buf[..]) {
            Ok(0) => Next::end(),
            Ok(n) => {
                println!("read {} bytes", n);
                (self.callback)(Vec::new());
                Next::read()
            },
            Err(e) => match e.kind() {
                io::ErrorKind::WouldBlock => Next::read(),
                _ => {
                    println!("ERROR: {}", e);
                    Next::end()
                }
            }
        }
    }

    fn on_error(&mut self, err: hyper::Error) -> Next {
        println!("ERROR: {}", err);
        Next::remove()
    }
}
