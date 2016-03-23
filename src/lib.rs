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
extern crate mime;
extern crate handlebars;
extern crate serde;
extern crate serde_json;

pub use hyper::header as header;
use header::{Cookie as CookieHeader, ContentLength, ContentType, Header, HeaderFormat, Location, SetCookie};
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::net::Fresh;
use hyper::server::{Handler, Server};
use hyper::server::Request as HttpRequest;
use hyper::server::Response as HttpResponse;

use mime::{Mime, TopLevel, SubLevel, Attr, Value};

use handlebars::Handlebars;
use serde::ser::Serialize as ToJson;

pub use serde_json::value as value;

use std::fmt::Debug;
use std::borrow::Cow;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::fs::{File, read_dir};
use std::path::Path;
use std::ops::Drop;

/// A request, with a path, query, and fragment (accessor methods not yet implemented for the last two).
///
/// Can be queried for the parameters that were matched by the router.
pub struct Request<'a, 'b: 'a> {
    inner: HttpRequest<'a, 'b>,
    path: Vec<String>,
    query: Option<String>,
    fragment: Option<String>,

    params: Option<Params>
}

type Params = Vec<(String, String)>;

impl<'a, 'b> Request<'a, 'b> {
    fn new(inner: HttpRequest<'a, 'b>) -> (Request<'a, 'b>, url::ParseResult<()>) {
        let ((path, query, fragment), parse_result) = match inner.uri {
            hyper::uri::RequestUri::AbsolutePath(ref path) => match url::parse_path(path) {
                Ok(res) => (res, Ok(())),
                Err(e) => ((Vec::new(), None, None), Err(e))
            },
            _ => ((vec!["*".to_owned()], None, None), Ok(()))
        };

        (Request {inner: inner, path: path, query: query, fragment: fragment, params: None}, parse_result)
    }

    /// Reads this request's body until the end, and returns it as a vector of bytes.
    pub fn body(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        try!(self.inner.read_to_end(&mut buf));
        Ok(buf)
    }

    /// Returns an iterator over the cookies of this request.
    pub fn cookies(&self) -> std::slice::Iter<Cookie> {
        self.inner.headers.get::<CookieHeader>().map_or([].iter(),
            |&CookieHeader(ref cookies)| cookies.iter()
        )
    }

    /// Reads the body of this request, parses it as an application/x-www-form-urlencoded format,
    /// and returns it as a vector of (name, value) pairs.
    pub fn form(&mut self) -> Result<Vec<(String, String)>> {
        match self.inner.headers.get::<ContentType>() {
            Some(&ContentType(Mime(TopLevel::Application, SubLevel::WwwFormUrlEncoded, _))) =>
                Ok(url::form_urlencoded::parse(&try!(self.body()))),
            Some(_) => Err(Error::new(ErrorKind::InvalidInput, "invalid Content-Type, expected application/x-www-form-urlencoded")),
            None => Err(Error::new(ErrorKind::InvalidInput, "missing Content-Type header"))
        }
    }

    /// Returns the parameters declared by the route that matched the URL of this request.
    pub fn params(&self) -> std::slice::Iter<(String, String)> {
        self.params.as_ref().map_or([].iter(), |params| params.iter())
    }

    /// Returns the path of this request, i.e. the list of segments of the URL.
    pub fn path(&self) -> &Vec<String> {
        &self.path
    }

    /// Returns the query of this request (if any).
    pub fn query(&self) -> &Option<String> {
        &self.query
    }

    /// Returns the fragment of this request (if any).
    pub fn fragment(&self) -> &Option<String> {
        &self.fragment
    }
}

/// Drop implementation to make sure the body of a request is discarded if nobody reads it.
impl<'a, 'b> Drop for Request<'a, 'b> {
    fn drop(&mut self) {
        // read the request body in case the callback did not read it
        // avoids a weird bug where Hyper does not correctly parse the method
        let mut buf = Vec::new();
        self.inner.read_to_end(&mut buf).unwrap();
    }
}

pub struct Response<'a> {
    inner: HttpResponse<'a>
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

impl<'a> Response<'a> {
    fn new(inner: HttpResponse<'a>) -> Response<'a> {
        Response {
            inner: inner
        }
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        *self.inner.status_mut() = status;
        self
    }

    /// Sets the Content-Type header.
    pub fn content_type<S: Into<Vec<u8>>>(&mut self, mime: S) -> &mut Self {
        self.header_raw("Content-Type", mime)
    }

    /// Sets the Content-Length header.
    pub fn len(&mut self, len: u64) -> &mut Self {
        self.header(ContentLength(len))
    }

    /// Sets the Location header.
    pub fn location<S: Into<String>>(&mut self, url: S) -> &mut Self {
        self.header(Location(url.into()))
    }

    /// Redirects to the given URL with the given status, or 302 Found if none is given.
    pub fn redirect(mut self, url: &'a str, status: Option<Status>) -> Result<()> {
        self.location(url);
        self.end(status.unwrap_or(Status::Found))
    }

    /// Sets the given header.
    pub fn header<H: Header + HeaderFormat>(&mut self, header: H) -> &mut Self {
        self.inner.headers_mut().set(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.inner.headers_mut().set_raw(name, vec![value.into()]);
        self
    }

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) -> Result<()> {
        self.status(status);
        self.send([])
    }

    pub fn render<P: AsRef<Path>, T: ToJson>(self, path: P, data: T) -> Result<()> {
        let mut handlebars = Handlebars::new();
        let path = path.as_ref();
        let name = path.file_stem().unwrap().to_str().unwrap();

        handlebars.register_template_file(name, path).unwrap();
        try!(register_partials(&mut handlebars));
        let result = handlebars.render(name, &data);
        self.send(result.unwrap())
    }

    /// Sends the given content and ends this response.
    /// Status defaults to 200 Ok, headers must have been set before this method is called.
    pub fn send<D: AsRef<[u8]>>(self, content: D) -> Result<()> {
        self.inner.send(content.as_ref())
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    /// Known extensions are htm, html, jpg, jpeg, png, js, css.
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(mut self, path: P) -> Result<()> {
        if !self.inner.headers().has::<ContentType>() {
            let extension = path.as_ref().extension();
            if let Some(ext) = extension {
                let content_type = match ext.to_string_lossy().as_ref() {
                    "htm" | "html" => Some(ContentType::html()),
                    "jpg" | "jpeg" => Some(ContentType::jpeg()),
                    "png" => Some(ContentType::png()),
                    "js" => Some(ContentType(Mime(TopLevel::Application, SubLevel::Javascript, vec![(Attr::Charset, Value::Utf8)]))),
                    "css" => Some(ContentType(Mime(TopLevel::Application, SubLevel::Css, vec![(Attr::Charset, Value::Utf8)]))),
                    _ => None
                };

                if let Some(content_type) = content_type {
                    self.inner.headers_mut().set(content_type);
                }
            }
        }

        // read the whole file at once and send it
        // probably not the best idea for big files, we should use stream instead in that case
        match File::open(path) {
            Ok(mut file) => {
                let mut buf = Vec::with_capacity(file.metadata().ok().map_or(1024, |meta| meta.len() as usize));
                if let Err(err) = file.read_to_end(&mut buf) {
                    self.status(Status::InternalServerError).content_type("text/plain");
                    self.send(format!("{}", err))
                } else {
                    self.send(buf)
                }
            },
            Err(ref err) if err.kind() == ErrorKind::NotFound => self.end(Status::NotFound),
            Err(ref err) => {
                self.status(Status::InternalServerError).content_type("text/plain");
                self.send(format!("{}", err))
            }
        }
    }

    /// Writes the body of this response using the given source function.
    pub fn stream<F, R>(self, source: F) -> Result<()> where F: FnOnce(&mut Write) -> Result<R> {
        let mut streaming = try!(self.inner.start());
        try!(source(&mut streaming));
        streaming.end()
    }

    /// Sets a cookie with the given name and value.
    /// If set, the set_options function will be called to update the cookie's options.
    pub fn cookie<F>(&mut self, name: &str, value: &str, set_options: Option<F>) where F: Fn(&mut Cookie) {
        let mut cookie = Cookie::new(name.to_owned(), value.to_owned());
        set_options.map(|f| f(&mut cookie));

        if self.inner.headers().has::<SetCookie>() {
            self.inner.headers_mut().get_mut::<SetCookie>().unwrap().push(cookie)
        } else {
            self.inner.headers_mut().set(SetCookie(vec![cookie]))
        }
    }
}

/// Signature for a callback method
pub type Callback<T> = fn(&T, &mut Request, Response) -> Result<()>;

/// A segment is either a fixed string, or a variable with a name
#[derive(Debug, Clone)]
enum Segment {
    Fixed(String),
    Variable(String)
}

/// A route is an absolute URL pattern with a leading slash, and segments separated by slashes.
///
/// A segment that begins with a colon declares a variable, for example "/:user_id".
#[derive(Debug)]
pub struct Route {
    segments: Vec<Segment>
}

/// Router structure
struct Router<T> {
    routes: Vec<(Route, Callback<T>)>
}

impl<T> Router<T> {
    fn new() -> Router<T> {
        Router {
            routes: Vec::new()
        }
    }

    /// Finds the first route (if any) that matches the given path, and returns the associated callback.
    fn find(&self, path: &Vec<String>) -> Option<(Params, Callback<T>)> {
        println!("path: {:?}", path);
        let mut params = Vec::new();
        'top: for &(ref route, ref callback) in self.routes.iter() {
            println!("route: {:?}", route);
            let mut it_route = route.segments.iter();
            for actual in path.iter() {
                match it_route.next() {
                    Some(&Segment::Fixed(ref fixed)) if fixed != actual => continue 'top,
                    Some(&Segment::Variable(ref name)) => {
                        params.push((name.to_owned(), actual.to_owned()));
                    },
                    _ => ()
                }
            }

            if it_route.next().is_none() {
                return Some((params, *callback));
            }
            params.clear();
        }
        None
    }

    fn insert(&mut self, route: Route, method: Callback<T>) {
        println!("register callback for route: {:?}", route);
        self.routes.push((route, method));
    }
}

/// Creates a Route from a &str.
impl<'a> Into<Route> for &'a str {
    fn into(self) -> Route {
        if self.len() == 0 {
            panic!("route must not be empty");
        }
        if &self[0..1] != "/" {
            panic!("route must begin with a slash");
        }

        let stripped = &self[1..];
        let route = Route {
            segments:
                stripped.split('/').map(|segment| if segment.len() > 0 && &segment[0..1] == ":" {
                        Segment::Variable(segment[1..].to_owned())
                    } else {
                        Segment::Fixed(segment.to_owned())
                    }
                ).collect::<Vec<Segment>>()
        };
        println!("into from {} to {:?}", self, route);
        route
    }
}

/// Container of an application.
pub struct Container<T: Send + Sync> {
    inner: T,
    router_get: Router<T>,
    router_post: Router<T>,
    router_put: Router<T>,
    router_delete: Router<T>,
    router_head: Router<T>
}

impl<T: 'static + Send + Sync> Container<T> {

    pub fn new(inner: T) -> Container<T> {
        Container {
            inner: inner,
            router_get: Router::new(),
            router_post: Router::new(),
            router_put: Router::new(),
            router_delete: Router::new(),
            router_head: Router::new()
        }
    }

    pub fn get<S: Into<Route>>(&mut self, path: S, method: Callback<T>) {
        self.router_get.insert(path.into(), method);
    }

    pub fn post<S: Into<Route>>(&mut self, path: S, method: Callback<T>) {
        self.router_post.insert(path.into(), method);
    }

    pub fn put<S: Into<Route>>(&mut self, path: S, method: Callback<T>) {
        self.router_put.insert(path.into(), method);
    }

    pub fn delete<S: Into<Route>>(&mut self, path: S, method: Callback<T>) {
        self.router_delete.insert(path.into(), method);
    }

    pub fn head<S: Into<Route>>(&mut self, path: S, method: Callback<T>) {
        self.router_head.insert(path.into(), method);
    }

    pub fn start(self, addr: &str) -> Result<()> {
        Server::http(addr).unwrap().handle(self).unwrap();
        Ok(())
    }

    fn find_callback<'a, 'k>(&'a self, req: &'a Request<'a, 'k>) -> Option<(Params, Callback<T>)> {
        use hyper::method::Method::*;

        let router = match req.inner.method {
            Get => &self.router_get,
            Post => &self.router_post,
            Put => &self.router_put,
            Delete => &self.router_delete,
            Head => &self.router_head,
            ref method => { println!("unexpected method: {}", method); return None }
        };

        router.find(&req.path)
    }
}

/// Implements Handler for our Container. Wraps the HTTP request/response in our own types.
impl<T: 'static + Send + Sync> Handler for Container<T> {
    fn handle<'a, 'k>(&'a self, req: HttpRequest<'a, 'k>, res: HttpResponse<'a, Fresh>) {
        let mut res = Response::new(res);

        // we do this so that req can be dropped (see Drop impl for Request)
        let (mut req, parse_result) = Request::new(req);

        let result = match parse_result {
            Err(parse_error) => {
                res.status(Status::BadRequest);
                res.content_type("text/plain");
                res.send(format!("{}", parse_error))
            },
            Ok(()) => {
                match self.find_callback(&req) {
                    None => res.end(Status::NotFound),
                    Some((params, f)) => {
                        req.params = Some(params);
                        f(&self.inner, &mut req, res)
                    }
                }
            }
        };
        result.unwrap();
    }
}
