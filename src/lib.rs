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

use std::sync::Arc;
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A request, with a path, query, and fragment (accessor methods not yet implemented for the last two).
///
/// Can be queried for the parameters that were matched by the router.
pub struct Request {
    inner: HttpRequest,
    path: Vec<String>,
    query: Option<String>,
    fragment: Option<String>,

    params: Option<Params>,
    body: Option<Buffer>
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

        Ok(Request {
            inner: inner,
            path: path,
            query: query,
            fragment: fragment,
            params: None,
            body: None})
    }

    /// Reads this request's body until the end, and returns it as a vector of bytes.
    pub fn body(&self) -> Result<&[u8]> {
        match self.body {
            Some(ref buffer) => Ok(&buffer.content),
            None => Err(Error::new(ErrorKind::UnexpectedEof, "empty body"))
        }
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
        let body = try!(self.body());

        match self.inner.headers().get::<ContentType>() {
            Some(&ContentType(Mime(TopLevel::Application, SubLevel::WwwFormUrlEncoded, _))) =>
                Ok(url::form_urlencoded::parse(body)),
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

struct Resp {
    status: Status,
    headers: hyper::Headers,
    body: Buffer
}

impl Resp {
    fn new() -> Resp {
        Resp {
            status: Status::default(),
            body: Buffer::new(),
            headers: hyper::Headers::default()
        }
    }

    fn deconstruct(self) -> (Status, hyper::Headers, Buffer) { (self.status, self.headers, self.body) }
}

pub struct Response {
    resp: Resp,
    ctrl: Control,
    tx: mpsc::Sender<Resp>,
    notify: Arc<AtomicBool>
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

impl Response {
    fn new(ctrl: Control, tx: mpsc::Sender<Resp>) -> Response {
        Response {
            resp: Resp::new(),
            ctrl: ctrl,
            tx: tx,
            notify: Arc::new(AtomicBool::new(false))
        }
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.resp.status = status;
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
    pub fn redirect(mut self, url: &str, status: Option<Status>) {
        self.location(url);
        self.end(status.unwrap_or(Status::Found))
    }

    /// Sets the given header.
    pub fn header<H: Header + HeaderFormat>(&mut self, header: H) -> &mut Self {
        self.resp.headers.set(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.resp.headers.set_raw(name, vec![value.into()]);
        self
    }

    /// Writes this response by notifying the framework
    fn write(self) {
        self.tx.send(self.resp).unwrap();
        if self.notify.load(Ordering::Relaxed) {
            self.ctrl.ready(Next::write()).unwrap();
        }
    }

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) {
        self.status(status);
        self.len(0);
        self.write();
    }

    pub fn render<P: AsRef<Path>, T: ToJson>(self, path: P, data: T) {
        let mut handlebars = Handlebars::new();
        let path = path.as_ref();
        let name = path.file_stem().unwrap().to_str().unwrap();

        handlebars.register_template_file(name, path).unwrap();
        register_partials(&mut handlebars).unwrap();
        let result = handlebars.render(name, &data);
        self.send(result.unwrap())
    }

    /// Sends the given content and ends this response.
    /// Status defaults to 200 Ok, headers must have been set before this method is called.
    pub fn send<D: Into<Vec<u8>>>(mut self, content: D) {
        self.resp.body.send(content);
        let length = self.resp.body.len() as u64;
        self.len(length);
        self.write();
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    /// Known extensions are htm, html, jpg, jpeg, png, js, css.
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(mut self, path: P) {
        if !self.resp.headers.has::<ContentType>() {
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
                    self.resp.headers.set(content_type);
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

        if self.resp.headers.has::<SetCookie>() {
            self.resp.headers.get_mut::<SetCookie>().unwrap().push(cookie)
        } else {
            self.resp.headers.set(SetCookie(vec![cookie]))
        }
    }
}

/// Signature for a callback method
pub type Callback<T> = fn(&T, &mut Request, Response);

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
    content: Vec<u8>,
    pos: usize,

    /// growable is either None when reading a fixed buffer (Content-Length known in advance)
    /// or it is Some(size) where size is the current write size by which the buffer should be grown
    /// every time it is full
    growable: Option<usize>
}

const DEFAULT_BUF_SIZE: usize = 8 * 1024;

impl Buffer {
    fn new() -> Buffer {
        Buffer {
            content: Vec::new(),
            pos: 0,
            growable: Some(16)
        }
    }

    fn with_capacity(capacity: usize) -> Buffer {
        println!("creating buffer with capacity {}", capacity);
        Buffer {
            content: vec![0; capacity],
            pos: 0,
            growable: None
        }
    }

    /// used when writing to check whether the buffer still has data
    fn is_empty(&self) -> bool { self.pos == self.len() }

    /// used when reading to check whether the buffer can still hold data
    fn is_full(&self) -> bool { self.pos == self.len() }

    /// returns the length of this buffer's content
    fn len(&self) -> usize { self.content.len() }

    /// read from the given reader into this buffer
    fn read<R: Read>(&mut self, reader: &mut R) -> Option<Next> {
        if let Some(mut write_size) = self.growable {
            // if buffer is growable, check whether it is full
            if self.is_full() {
                let len = self.len();
                // if buffer is full, extend it
                // reused Read::read_to_end algorithm
                if write_size < DEFAULT_BUF_SIZE {
                    write_size *= 2;
                    self.growable = Some(write_size);
                }
                self.content.resize(len + write_size, 0);
            }
        }

        match reader.read(&mut self.content[self.pos..]) {
            Ok(0) => {
                // note: EOF is supposed to happen only when reading a growable buffer
                if self.growable.is_some() {
                    // we truncate the buffer so it has the proper size
                    self.content.truncate(self.pos);
                }
                None
            }
            Ok(n) => {
                self.pos += n;
                if self.growable.is_none() && self.is_full() {
                    // fixed size full buffer, nothing to read anymore
                    None
                } else {
                    // fixed size buffer not full, or growable buffer
                    Some(Next::read())
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock => Some(Next::read()),
                _ => {
                    println!("read error {:?}", e);
                    Some(Next::end())
                }
            }
        }
    }

    fn send<D: Into<Vec<u8>>>(&mut self, content: D) {
        self.content = content.into();
    }

    /// writes from this buffer into the given writer
    fn write<W: Write>(&mut self, writer: &mut W) -> Next {
        match writer.write(&self.content[self.pos..]) {
            Ok(0) => panic!("wrote 0 bytes"),
            Ok(n) => {
                println!("wrote {} bytes", n);
                self.pos += n;
                if self.is_empty() {
                    // done reading
                    Next::end()
                } else {
                    Next::write()
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::WouldBlock => Next::write(),
                _ => {
                    println!("write error {:?}", e);
                    Next::end()
                }
            }
        }
    }
}

pub struct EdgeHandler<T: Send + Sync> {
    inner: Arc<Container<T>>,
    request: Option<Request>,
    body: Option<Buffer>,
    response: Option<Response>,
    resp: Option<Resp>,
    rx: mpsc::Receiver<Resp>
}

impl<T: 'static + Send + Sync> HandlerFactory<HttpStream> for Edge<T> {
    type Output = EdgeHandler<T>;

    fn create(&mut self, control: Control) -> EdgeHandler<T> {
        let (tx, rx) = mpsc::channel();
        EdgeHandler {
            inner: self.container.clone(),
            request: None,
            body: None,
            rx: rx,
            response: Some(Response::new(control, tx)),
            resp: None
        }
    }
}

impl<T: 'static + Send + Sync> EdgeHandler<T> {
    fn callback(&mut self) -> Next {
        let req = &mut self.request.as_mut().unwrap();
        let mut res = self.response.take().unwrap();
        let notify = res.notify.clone();

        if let Some((params, callback)) = self.inner.find_callback(req) {
            req.params = Some(params);

            callback(&self.inner.inner, req, res);
        } else {
            println!("route not found for path {:?}", req.path());
            res.status(Status::NotFound);
            res.content_type("text/plain");
            res.send(format!("not found: {:?}", req.path()));
        }

        match self.rx.try_recv() {
            Ok(resp) => {
                // try_recv only succeeds if response available
                self.resp = Some(resp);
                println!("response done, return Next::write after callback");
                Next::write()
            }
            Err(mpsc::TryRecvError::Empty) => {
                // otherwise we ask the Response to notify us, and wait
                notify.store(true, Ordering::Relaxed);
                println!("response not done, return Next::wait after callback");
                Next::wait()
            }
            Err(_) => panic!("channel unexpectedly disconnected")
        }
    }
}

/// Implements Handler for our EdgeHandler.
impl<T: 'static + Send + Sync> Handler<HttpStream> for EdgeHandler<T> {
    fn on_request(&mut self, req: HttpRequest) -> Next {
        println!("on_request");

        match Request::new(req) {
            Ok(mut req) => {
                match *req.inner.method() {
                    Put | Post => {
                        // need to read body
                        req.body = Some(match req.inner.headers().get::<ContentLength>() {
                            Some(&ContentLength(len)) => Buffer::with_capacity(len as usize),
                            None => Buffer::new()
                        });
                        self.request = Some(req);
                        Next::read()
                    }
                    _ => {
                        self.request = Some(req);
                        self.callback()
                    }
                }
            },
            Err(error) => {
                let mut res = self.response.take().unwrap();
                res.status(Status::BadRequest);
                res.content_type("text/plain");
                res.send(error.to_string());
                Next::write()
            }
        }
    }

    fn on_request_readable(&mut self, transport: &mut Decoder<HttpStream>) -> Next {
        println!("on_request_readable");

        // we can only get here if self.request = Some(...), or there is a bug
        {
            let req = self.request.as_mut().unwrap();
            let body = req.body.as_mut().unwrap();
            if let Some(res) = body.read(transport) {
                return res
            }
        }

        self.callback()
    }

    fn on_response(&mut self, res: &mut HttpResponse) -> Next {
        println!("on_response");

        // we got here from callback directly or Response notified the Control
        // in first case, we have a resp, in second case we need to recv it
        let resp = self.resp.take().unwrap_or_else(|| self.rx.recv().unwrap());

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
