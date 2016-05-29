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
//! struct Hello;
//! impl Hello {
//!     fn hello(&self, _req: &mut Request, mut res: Response) {
//!         res.content_type("text/plain");
//!         res.send("Hello, world!")
//!     }
//! }
//!
//! fn main() {
//!     let mut edge = Edge::new("0.0.0.0:3000", Hello);
//!     edge.get("/", Hello::hello);
//!     edge.start().unwrap();
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
//! struct AsyncHello;
//! impl AsyncHello {
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
//!     let mut edge = Edge::new("0.0.0.0:3000", AsyncHello);
//!     edge.get("/", AsyncHello::hello);
//!     edge.start().unwrap();
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
//! struct Templating {
//!     version: &'static str
//! }
//!
//! impl Templating {
//!     fn page_handler(&self, req: &mut Request, mut res: Response) {
//!         let mut data = BTreeMap::new();
//!         data.insert("title", req.param("page").unwrap());
//!         data.insert("version", self.version);
//!
//!         res.content_type("text/html").header(Server(format!("Edge version {}", self.version)));
//!         res.render("tmpl", data)
//!     }
//! }
//!
//! fn main() {
//!     let app = Templating { version: "0.1" };
//!     let mut edge = Edge::new("0.0.0.0:3000", app);
//!     edge.get("/:page", Templating::page_handler);
//!     edge.register_template("tmpl");
//!     edge.start().unwrap();
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
//! struct Counting {
//!     counter: AtomicUsize
//! }
//!
//! impl Counting {
//!     fn new() -> Counting { Counting { counter: AtomicUsize::new(0) } }
//!
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
//!     let mut cter = Edge::new("0.0.0.0:3000", Counting::new());
//!     cter.get("/", Counting::home);
//!     cter.start().unwrap();
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

use handlebars::Handlebars;

use header::ContentLength;

use hyper::{Client as HttpClient, Decoder, Encoder, Method, Next};
use hyper::client::{Request as ClientRequest, Response as ClientResponse};
use hyper::method::Method::{Delete, Get, Head, Post, Put};
use hyper::net::HttpStream;
use hyper::server::Server;

use std::fs::read_dir;
use std::io::{Read, Result};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
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
use handler::EdgeShared;
use router::Router;

/// Structure for an Edge application.
pub struct Edge<T> {
    shared: Arc<EdgeShared<T>>,
    handlebars: Arc<Handlebars>
}

impl<T> Edge<T> {

    /// Creates an Edge application using the given address and application.
    pub fn new(addr: &str, app: T) -> Edge<T> {
        Edge {
            shared: Arc::new(EdgeShared {
                app: app,
                router: Router::new(addr)
            }),
            handlebars: Arc::new(Handlebars::new())
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
        let ref mut router = Arc::get_mut(&mut self.shared).unwrap().router;
        router.insert(method, path, callback)
    }

    // Registers a template with the given name.
    pub fn register_template(&mut self, name: &str) {
        let mut path = PathBuf::new();
        path.push("views");
        path.push(name);
        path.set_extension("hbs");

        let handlebars = Arc::get_mut(&mut self.handlebars).unwrap();
        handlebars.register_template_file(name, &path).unwrap();
    }

    /// Runs the server and never returns.
    ///
    /// This will block the current thread.
    pub fn start(mut self) -> Result<()> {
        // register partials folder (if it exists)
        let partials = Path::new("views/partials");
        if partials.exists() {
            let handlebars = Arc::get_mut(&mut self.handlebars).unwrap();
            register_partials(handlebars).unwrap();
        }

        // get address and start listening
        let addr = self.shared.router.base_url.to_socket_addrs().unwrap().next().unwrap();
        let server = Server::http(&addr).unwrap();

        // configure handler
        let (listening, server) = server.handle(move |control| {
            debug!("creating new edge handler");

            handler::EdgeHandler::new(self.shared.clone(), self.handlebars.clone(), control)
        }).unwrap();

        info!("Listening on http://{}", listening);
        server.run();
        Ok(())
    }
}

fn register_partials(handlebars: &mut Handlebars) -> Result<()> {
    for it in try!(read_dir("views/partials")) {
        let entry = try!(it);
        let path = entry.path();
        if path.extension().is_some() && path.extension().unwrap() == "hbs" {
            let name = path.file_stem().unwrap().to_str().unwrap();
            handlebars.register_template_file(name, path.as_path()).unwrap();
        }
    }
    Ok(())
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
