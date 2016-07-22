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
//! #[macro_use]
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response, Result, Router};
//!
//! #[derive(Default)]
//! struct Hello;
//! impl Hello {
//!     fn hello(&mut self, _req: &Request, res: &mut Response) -> Result {
//!         res.content_type("text/plain");
//!         ok!("Hello, world!")
//!     }
//! }
//!
//! fn main() {
//!     let mut edge = Edge::new("0.0.0.0:3000");
//!     let mut router = Router::new();
//!     router.get("/", Hello::hello);
//!     edge.mount("/", router);
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
//! #[macro_use]
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response, Result, Router, Status};
//! use std::thread;
//! use std::time::Duration;
//!
//! #[derive(Default)]
//! struct AsyncHello;
//! impl AsyncHello {
//!     fn hello(&mut self, _req: &Request, res: &mut Response) -> Result {
//!         println!("waiting 1 second");
//!         thread::sleep(Duration::from_secs(1));
//!
//!         res.content_type("text/plain");
//!         ok!(Status::Ok, "Hello, world!")
//!     }
//! }
//!
//! fn main() {
//!     let mut edge = Edge::new("0.0.0.0:3000");
//!     let mut router = Router::new();
//!     router.get("/", AsyncHello::hello);
//!     edge.mount("/", router);
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
//! #[macro_use]
//! extern crate edge;
//!
//! use edge::{Edge, Request, Response, Result, Router, Status};
//! use edge::header::Server;
//! use std::collections::BTreeMap;
//!
//! const VERSION: &'static str = "0.3";
//!
//! #[derive(Default)]
//! struct Templating;
//! impl Templating {
//!     fn page_handler(&mut self, req: &Request, res: &mut Response) -> Result {
//!         let mut data = BTreeMap::new();
//!         data.insert("title", req.param("page"));
//!         data.insert("version", Some(VERSION));
//!
//!         res.content_type("text/html").header(Server(format!("Edge version {}", VERSION)));
//!         ok!("tmpl", data)
//!     }
//! }
//!
//! fn main() {
//!     let mut edge = Edge::new("0.0.0.0:3000");
//!     let mut router = Router::new();
//!     router.get("/:page", Templating::page_handler);
//!     edge.mount("/", router);
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
//! ```

extern crate crossbeam;
extern crate handlebars;
extern crate hyper;
extern crate num_cpus;
extern crate pulldown_cmark;
extern crate scoped_pool;
extern crate serde;
extern crate url;

#[macro_use]
extern crate log;
pub extern crate serde_json;

pub use hyper::header as header;
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

/// serde_json crate
pub use serde_json as json;

use handlebars::{Context, Handlebars, Helper, RenderContext, RenderError};

use hyper::net::HttpListener;
use hyper::server::Server;

use pulldown_cmark::Parser;
use pulldown_cmark::{Options, OPTION_ENABLE_TABLES, OPTION_ENABLE_FOOTNOTES};
use pulldown_cmark::html;

use scoped_pool::Pool;

use url::Url;

use std::fs::read_dir;
use std::io::Result as IoResult;
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::result;

mod buffer;
mod client;
mod handler;
mod router;
mod request;
mod response;

pub use client::Client;
pub use request::Request;
pub use response::{Response, Result, Action, stream};
pub use router::{Router};

/// Structure for an Edge application.
pub struct Edge {
    base_url: Url,
    routers: Vec<router::RouterAny>,
    handlebars: Handlebars
}

/// ok!() means Ok(Action::End).
/// ok!(expr) returns Ok(From::from(expr))
#[macro_export]
macro_rules! ok {
    () => (
        return Ok($crate::Action::End(None));
    );
    ($exp:expr) => (
        return Ok(::std::convert::From::from($exp));
    );
    ($e1:expr, $e2:expr) => (
        return Ok(::std::convert::From::from(($e1, $e2)));
    )
}

impl Edge {

    /// Creates an Edge application using the given address and application.
    pub fn new(addr: &str) -> Edge {
        let mut handlebars = Handlebars::new();
        init_handlebars(&mut handlebars).unwrap();

        Edge {
            base_url: Url::parse(&("http://".to_string() + addr)).unwrap(),
            routers: Vec::new(),
            handlebars: handlebars
        }
    }

    /// Mounts the given router at the given path.
    ///
    /// Use "/" to mount the router at the root.
    pub fn mount<T>(&mut self, mount: &str, router: Router<T>) {
        let mut router = router::get_inner(router);
        router.set_prefix(mount);
        self.routers.push(router)
    }

    // Registers a template with the given name.
    pub fn register_template(&mut self, name: &str) {
        let mut path = PathBuf::new();
        path.push("views");
        path.push(name);
        path.set_extension("hbs");

        self.handlebars.register_template_file(name, &path).unwrap();
    }

    /// Runs the server in one thread per cpu.
    ///
    /// Creates one instance of `T` per request by calling `Default::default`.
    /// This method blocks the current thread.
    pub fn start(&mut self) -> IoResult<()> {
        assert!(!self.routers.is_empty(), "No router registered! Please mount at least one router");

        // get address and start listening
        let addr = self.base_url.to_socket_addrs().unwrap().next().unwrap();
        let listener = HttpListener::bind(&addr).unwrap();

        let num_cpus = num_cpus::get();
        let pool = Pool::new(num_cpus * 4);
        pool.scoped(|pool_scope| {
            crossbeam::scope(|scope| {
                for i in 0..num_cpus {
                    let listener = listener.try_clone().unwrap();
                    let base_url = &self.base_url;
                    let routers = &self.routers;
                    let handlebars = &self.handlebars;
                    scope.spawn(move || {
                        info!("thread {} listening on http://{}", i, addr);
                        Server::new(listener).handle(move |control| {
                            handler::EdgeHandler::new(pool_scope, &base_url, &routers, &handlebars, control)
                        }).unwrap();
                    });
                }
            });
        });

        Ok(())
    }
}

fn render_html(text: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(OPTION_ENABLE_TABLES);
    opts.insert(OPTION_ENABLE_FOOTNOTES);

    let mut s = String::with_capacity(text.len() * 3 / 2);
    let p = Parser::new_ext(text, opts);
    html::push_html(&mut s, p);
    s
}

/// this code is based on code Copyright (c) 2015 Wayne Nilsen
/// see https://github.com/waynenilsen/handlebars-markdown-helper/blob/master/src/lib.rs#L31
///
/// because the handlebars-markdown-helper crate does not allow custom options for Markdown rendering yet
fn markdown_helper(_: &Context, h: &Helper, _ : &Handlebars, rc: &mut RenderContext) -> result::Result<(), RenderError> {
    let markdown_text_var = try!(h.param(0).ok_or_else(|| RenderError::new(
        "Param not found for helper \"markdown\"")
    ));
    let markdown = try!(markdown_text_var.value().as_string().ok_or_else(||
        RenderError::new(format!("Expected a string for parameter {:?}", markdown_text_var))
    ));
    let html = render_html(markdown);
    try!(rc.writer.write_all(html.as_bytes()));
    Ok(())
}

fn init_handlebars(handlebars: &mut Handlebars) -> IoResult<()> {
    // register markdown helper
    handlebars.register_helper("markdown", Box::new(::markdown_helper));

    // register partials folder (if it exists)
    let partials = Path::new("views/partials");
    if partials.exists() {
        for it in try!(read_dir("views/partials")) {
            let entry = try!(it);
            let path = entry.path();
            if path.extension().is_some() && path.extension().unwrap() == "hbs" {
                let name = path.file_stem().unwrap().to_str().unwrap();
                handlebars.register_template_file(name, path.as_path()).unwrap();
            }
        }
    }

    Ok(())
}
