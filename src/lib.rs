extern crate hyper;
extern crate url;
extern crate mime;

use hyper::header::Cookie as CookieHeader;
use hyper::header::{ContentLength, ContentType, SetCookie};
pub use hyper::header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::net::Fresh;
use hyper::server::{Handler, Server};
use hyper::server::Request as HttpRequest;
use hyper::server::Response as HttpResponse;

use mime::{Mime, TopLevel, SubLevel};

use std::io::{Error, ErrorKind, Read, Result, Write};
use std::ops::Drop;

pub struct Request<'a, 'b: 'a> {
    inner: HttpRequest<'a, 'b>,
    path: Vec<String>,
    query: Option<String>,
    fragment: Option<String>
}

impl<'a, 'b> Request<'a, 'b> {
    fn new(inner: HttpRequest<'a, 'b>) -> (Request<'a, 'b>, url::ParseResult<()>) {
        let ((path, query, fragment), parse_result) = match inner.uri {
            hyper::uri::RequestUri::AbsolutePath(ref path) => match url::parse_path(path) {
                Ok(res) => (res, Ok(())),
                Err(e) => ((Vec::new(), None, None), Err(e))
            },
            _ => ((vec!["*".to_owned()], None, None), Ok(()))
        };

        (Request {inner: inner, path: path, query: query, fragment: fragment}, parse_result)
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
}

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

impl<'a> Response<'a> {
    fn new(inner: HttpResponse<'a>) -> Response<'a> {
        Response {
            inner: inner
        }
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

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) -> Result<()> {
        self.set_status(status);
        self.send([])
    }

    /// Sends the given content and ends this response.
    /// Status defaults to 200 Ok, headers must have been set before this method is called.
    pub fn send<D: AsRef<[u8]>>(self, content: D) -> Result<()> {
        self.inner.send(content.as_ref())
    }

    /// Sets the Content-Length header.
    pub fn set_len(&mut self, len: u64) {
        self.inner.headers_mut().set(ContentLength(len))
    }

    /// Sets the status code of this response.
    pub fn set_status(&mut self, status: Status) {
        *self.inner.status_mut() = status
    }

    /// Sets the Content-Type header.
    pub fn set_type<S: Into<Vec<u8>>>(&mut self, mime: S) {
        self.inner.headers_mut().set_raw("Content-Type", vec![mime.into()])
    }

    /// Writes the body of this response using the given source function.
    pub fn stream<F, R>(self, source: F) -> Result<()> where F: FnOnce(&mut Write) -> Result<R> {
        let mut streaming = try!(self.inner.start());
        try!(source(&mut streaming));
        streaming.end()
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

/// A route is something like "fixed/:some_var/:another_var".
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
    fn find(&self, path: &Vec<String>) -> Option<Callback<T>> {
        println!("path: {:?}", path);
        'top: for &(ref route, ref callback) in self.routes.iter() {
            println!("route: {:?}", route);
            let mut it_route = route.segments.iter();
            for actual in path.iter() {
                match it_route.next() {
                    Some(&Segment::Fixed(ref fixed)) if fixed != actual => continue 'top,
                    _ => ()
                }
            }

            if it_route.next().is_none() {
                return Some(*callback);
            }
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
        let route = Route {
            segments:
                self.split('/').map(|segment| if segment.len() > 0 && &segment[0..1] == ":" {
                        Segment::Variable(segment.to_owned())
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

    fn find_callback<'a, 'k>(&'a self, req: &'a Request<'a, 'k>) -> Option<Callback<T>> {
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
        match parse_result {
            Err(parse_error) => {
                res.set_status(Status::BadRequest);
                res.send(format!("{}", parse_error)).unwrap();
            },
            Ok(()) => {
                match self.find_callback(&req) {
                    None => res.set_status(Status::NotFound),
                    Some(f) => f(&self.inner, &mut req, res).unwrap()
                }
            }
        }
    }
}
