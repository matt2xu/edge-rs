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
use header::{Cookie as CookieHeader, ContentLength, ContentType, Header, HeaderFormat, Location, SetCookie};
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::{Control, Decoder, Encoder, Next, Get, Post, Head, Delete};

use hyper::method::Method::{Put};
use hyper::uri::RequestUri::{AbsolutePath, Star};

use hyper::net::HttpStream;
use hyper::server::{Handler, HandlerFactory, Server};
use hyper::server::{Request as HttpRequest, Response as HttpResponse};

use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};

use handlebars::Handlebars;
use serde::ser::Serialize as ToJson;

pub use serde_json::value as value;

use std::fmt::Debug;
use std::borrow::Cow;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::fs::{File, read_dir};
use std::path::Path;

/// A request, with a path, query, and fragment (accessor methods not yet implemented for the last two).
///
/// Can be queried for the parameters that were matched by the router.
pub struct Request {
    inner: HttpRequest,
    path: Vec<String>,
    query: Option<String>,
    fragment: Option<String>,

    params: Option<Params>
}

type Params = Vec<(String, String)>;

impl Request {
    fn new(inner: HttpRequest) -> url::ParseResult<Request> {
        let (path, query, fragment) = match *inner.uri() {
            AbsolutePath(ref path) => match url::parse_path(path) {
                Ok(res) => res,
                Err(e) => return Err(e)
            },
            Star => (vec!["*".to_owned()], None, None),
            _ => panic!("unsupported request URI")
        };

        Ok(Request {inner: inner, path: path, query: query, fragment: fragment, params: None})
    }

    /// Reads this request's body until the end, and returns it as a vector of bytes.
    pub fn body(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        // TODO try!(self.inner.read_to_end(&mut buf));
        Ok(buf)
    }

    /// Returns an iterator over the cookies of this request.
    pub fn cookies(&self) -> std::slice::Iter<Cookie> {
        self.inner.headers().get::<CookieHeader>().map_or([].iter(),
            |&CookieHeader(ref cookies)| cookies.iter()
        )
    }

    /// Reads the body of this request, parses it as an application/x-www-form-urlencoded format,
    /// and returns it as a vector of (name, value) pairs.
    pub fn form(&mut self) -> Result<Vec<(String, String)>> {
        match self.inner.headers().get::<ContentType>() {
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

pub struct Response<'a, 'b: 'a> {
    inner: &'a mut HttpResponse<'b>,
    buffer: &'a mut Buffer
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

impl<'a, 'b> Response<'a, 'b> {
    fn new(inner: &'a mut HttpResponse<'b>, buffer: &'a mut Buffer) -> Response<'a, 'b> {
        Response {
            inner: inner,
            buffer: buffer
        }
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.inner.set_status(status);
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
    pub fn redirect(&mut self, url: &str, status: Option<Status>) -> Result<()> {
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
    pub fn end(&mut self, status: Status) -> Result<()> {
        self.status(status);
        self.len(0);
        Ok(())
    }

    pub fn render<P: AsRef<Path>, T: ToJson>(&mut self, path: P, data: T) -> Result<()> {
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
    pub fn send<D: Into<Vec<u8>>>(&mut self, content: D) -> Result<()> {
        self.buffer.send(content);
        let length = self.buffer.len() as u64;
        self.len(length);
        Ok(())
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    /// Known extensions are htm, html, jpg, jpeg, png, js, css.
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        if !self.inner.headers().has::<ContentType>() {
            let extension = path.as_ref().extension();
            if let Some(ext) = extension {
                let content_type = match ext.to_string_lossy().as_ref() {
                    "htm" | "html" => Some(ContentType::html()),
                    "jpg" | "jpeg" => Some(ContentType::jpeg()),
                    "png" => Some(ContentType::png()),
                    "js" => Some(ContentType(Mime(TopLevel::Text, SubLevel::Javascript, vec![(Attr::Charset, Value::Utf8)]))),
                    "css" => Some(ContentType(Mime(TopLevel::Text, SubLevel::Css, vec![(Attr::Charset, Value::Utf8)]))),
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
    pub fn stream<F, R>(&mut self, source: F) -> Result<()> where F: FnOnce(&mut Write) -> Result<R> {
        //let mut streaming = try!(self.inner.start());
        //try!(source(&mut streaming));
        //streaming.end()
        Ok(())
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
pub type Callback<T> = fn(&T, &mut Request, &mut Response) -> Result<()>;

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
struct Route {
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

/// Structure for an Edge application.
pub struct Edge<T: Send + Sync> {
    container: Arc<Container<T>>
}

use std::sync::Arc;

impl<T: 'static + Send + Sync> Edge<T> {

    /// Creates an Edge application using the given inner structure.
    pub fn new(inner: T) -> Edge<T> {
        Edge {
            container: Arc::new(Container {
                inner: inner,
                router_get: Router::new(),
                router_post: Router::new(),
                router_put: Router::new(),
                router_delete: Router::new(),
                router_head: Router::new()
            })
        }
    }

    pub fn get(&mut self, path: &str, method: Callback<T>) {
        self.container_mut().router_get.insert(path.into(), method);
    }

    pub fn post(&mut self, path: &str, method: Callback<T>) {
        self.container_mut().router_post.insert(path.into(), method);
    }

    pub fn put(&mut self, path: &str, method: Callback<T>) {
        self.container_mut().router_put.insert(path.into(), method);
    }

    pub fn delete(&mut self, path: &str, method: Callback<T>) {
        self.container_mut().router_delete.insert(path.into(), method);
    }

    pub fn head(&mut self, path: &str, method: Callback<T>) {
        self.container_mut().router_head.insert(path.into(), method);
    }

    pub fn start(self, addr: &str) -> Result<()> {
        let server = Server::http(&addr.parse().unwrap()).unwrap();
        server.handle(self).unwrap();
        Ok(())
    }

    fn container_mut(&mut self) -> &mut Container<T> {
        Arc::get_mut(&mut self.container).unwrap()
    }

}

/// Container of an application.
struct Container<T: Send + Sync> {
    inner: T,
    router_get: Router<T>,
    router_post: Router<T>,
    router_put: Router<T>,
    router_delete: Router<T>,
    router_head: Router<T>
}

impl<T: 'static + Send + Sync> Container<T> {
    fn find_callback(&self, req: &Request) -> Option<(Params, Callback<T>)> {
        let router = match req.inner.method() {
            &Get => &self.router_get,
            &Post => &self.router_post,
            &Put => &self.router_put,
            &Delete => &self.router_delete,
            &Head => &self.router_head,
            ref method => { println!("unexpected method: {}", method); return None }
        };

        router.find(&req.path)
    }
}

struct Buffer {
    content: Option<Vec<u8>>,
    pos: usize
}

impl Buffer {
    fn new() -> Buffer {
        Buffer {
            content: None,
            pos: 0
        }
    }

    fn len(&self) -> usize {
        self.content.as_ref().unwrap().len()
    }

    fn send<D: Into<Vec<u8>>>(&mut self, content: D) {
        self.content = Some(content.into());
    }

    fn write<W: Write>(&self, writer: &mut W) -> Result<usize> {
        writer.write(&self.content.as_ref().unwrap()[self.pos..])
    }
}

pub struct EdgeHandler<T: Send + Sync> {
    inner: Arc<Container<T>>,
    request: Option<Request>,
    error: Option<String>,

    /// used for response
    body: Buffer,

    callback: Option<Callback<T>>,
    ctrl: Control
}

impl<T: 'static + Send + Sync> HandlerFactory<HttpStream> for Edge<T> {
    type Output = EdgeHandler<T>;

    fn create(&mut self, control: Control) -> EdgeHandler<T> {
        EdgeHandler {
            inner: self.container.clone(),
            request: None,
            error: None,

            body: Buffer::new(),

            callback: None,
            ctrl: control
        }
    }
}

/// Implements Handler for our EdgeHandler.
impl<T: 'static + Send + Sync> Handler<HttpStream> for EdgeHandler<T> {
    fn on_request(&mut self, req: HttpRequest) -> Next {
        match Request::new(req) {
            Ok(mut req) => {
                if let Some((params, callback)) = self.inner.find_callback(&req) {
                    req.params = Some(params);
                    self.callback = Some(callback);
                } else {
                    println!("route not found for path {:?}", req.path());
                }

                let need_reading = match *req.inner.method() { Put | Post => true, _ => false };
                self.request = Some(req);
                if need_reading {
                    // need to read body (if any)
                    return Next::read_and_write()
                }
            },
            Err(e) => {
                self.error = Some(format!("{}", e));
            }
        }

        // if request other than PUT/POST or no body, write a response
        Next::write()
    }

    fn on_request_readable(&mut self, transport: &mut Decoder<HttpStream>) -> Next {
        println!("on_request_readable");

        // TODO read request body here, repeatedly returning Next::read()
        // when finished with reading body, return Next::write()
        Next::write()
    }

    fn on_response(&mut self, res: &mut HttpResponse) -> Next {
        println!("on_response");

        if let Some(callback) = self.callback {
            let mut res = Response::new(res, &mut self.body);
            if let Err(e) = callback(&self.inner.inner, self.request.as_mut().unwrap(), &mut res) {
                self.error = Some(e.to_string());
            }

            Next::write()
        } else {
            let (status, content) = if let Some(ref error) = self.error {
                    (Status::BadRequest, error.to_string())
                } else {
                    (Status::NotFound, "not found".to_string())
                };
            res.set_status(status);
            self.body.send(content);
            Next::write()
        }
    }

    fn on_response_writable(&mut self, transport: &mut Encoder<HttpStream>) -> Next {
        if self.body.pos < self.body.len() {
            // repeatedly write the body here with Next::write
            match self.body.write(transport) {
                Ok(0) => panic!("wrote 0 bytes"),
                Ok(n) => {
                    self.body.pos += n;
                    Next::write()
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::WouldBlock => Next::write(),
                    _ => {
                        println!("write error {:?}", e);
                        Next::end()
                    }
                }
            }
        } else {
            // done writing the buffer
            Next::end()
        }
    }
}
