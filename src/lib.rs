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

use hyper::{Client as HttpClient, Decoder, Encoder, Method, Next};
use hyper::client::{Request as ClientRequest, Response as ClientResponse};
use hyper::method::Method::{Delete, Get, Head, Post, Put};
use hyper::net::HttpStream;
use hyper::server::Server;

use std::io::{Read, Result};
use std::net::SocketAddr;
use std::sync::{Arc};
use std::thread::{self, Thread};

mod buffer;
mod handler;
mod router;
mod request;
mod response;

pub use request::Request;
pub use response::{Response, Fresh, Streaming};
pub use router::Callback;

use buffer::Buffer;
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
        router.insert(method, path, callback)
    }

    /// Runs the server and never returns.
    ///
    /// This will block the current thread.
    pub fn start(self) -> Result<()> {
        let server = Server::http(&self.addr).unwrap();
        let (listening, server) = server.handle(move |control| {
            debug!("creating new edge handler");

            handler::EdgeHandler::new(self.router.clone(), self.inner.clone(), control)
        }).unwrap();

        info!("Listening on http://{}", listening);
        server.run();
        Ok(())
    }
}

pub struct Client {
    result: RequestResult
}

struct RequestResult {
    body: Option<Vec<u8>>,
    response: Option<ClientResponse>
}

impl RequestResult {
    fn new() -> RequestResult {
        RequestResult {
            body: None,
            response: None
        }
    }
}

impl Client {
    pub fn new() -> Client {
        Client {
            result: RequestResult::new()
        }
    }

    pub fn request(&mut self, url: &str) -> Vec<u8> {
        let client = HttpClient::new().unwrap();
        let _ = client.request(url.parse().unwrap(), ClientHandler::new(&mut self.result));

        // wait for request to complete
        thread::park();

        // close client and returns request body
        client.close();

        if let Some(buffer) = self.result.body.take() {
            buffer
        } else {
            Vec::new()
        }
    }

    pub fn status(&self) -> Status {
        *self.result.response.as_ref().unwrap().status()
    }
}

struct ClientHandler {
    thread: Thread,
    buffer: Buffer,
    result: *mut RequestResult
}

unsafe impl Send for ClientHandler {}

impl ClientHandler {
    fn new(result: &mut RequestResult) -> ClientHandler {
        ClientHandler {
            thread: thread::current(),
            buffer: Buffer::new(),
            result: result as *mut RequestResult
        }
    }
}

impl Drop for ClientHandler {
    fn drop(&mut self) {
        unsafe { (*self.result).body = Some(self.buffer.take()); }

        // unlocks waiting thread
        self.thread.unpark();
    }
}

impl hyper::client::Handler<HttpStream> for ClientHandler {

    fn on_request(&mut self, _req: &mut ClientRequest) -> Next {
        Next::read()
    }

    fn on_request_writable(&mut self, _encoder: &mut Encoder<HttpStream>) -> Next {
        Next::read()
    }

    fn on_response(&mut self, res: ClientResponse) -> Next {
        if let Some(&ContentLength(len)) = res.headers().get::<ContentLength>() {
            self.buffer.set_capacity(len as usize);
        }
        unsafe { (*self.result).response = Some(res); }

        Next::read()
    }

    fn on_response_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
        if let Ok(keep_reading) = self.buffer.read_from(decoder) {
            if keep_reading {
                return Next::read();
            }
        }

        Next::end()
    }

    fn on_error(&mut self, err: hyper::Error) -> Next {
        println!("ERROR: {}", err);
        Next::remove()
    }

}
