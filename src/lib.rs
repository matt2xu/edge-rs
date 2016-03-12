extern crate hyper;
extern crate url;

use hyper::header::Cookie as CookieHeader;
use hyper::header::{ContentLength, SetCookie};
pub use hyper::header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::net::Fresh;
use hyper::server::{Handler, Server};
use hyper::server::Request as HttpRequest;
use hyper::server::Response as HttpResponse;

use std::collections::HashMap;
use std::io::{Read, Result, Write};

pub struct Request<'a, 'b: 'a> {
    inner: HttpRequest<'a, 'b>
}

impl<'a, 'b> Request<'a, 'b> {
    fn new(inner: HttpRequest<'a, 'b>) -> Request<'a, 'b> {
        Request {
            inner: inner
        }
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
    /// TODO: should check that Content-Type is actually application/x-www-form-urlencoded, and return an error otherwise
    pub fn form(&mut self) -> Result<Vec<(String, String)>> {
        Ok(url::form_urlencoded::parse(&try!(self.body())))
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

struct Router<T> {
    paths: HashMap<String, Callback<T>>
}

impl<T> Router<T> {
    fn new() -> Router<T> {
        Router {
            paths: HashMap::new()
        }
    }

    fn find(&self, path: &String) -> Option<Callback<T>> {
        println!("path: {}", path);
        self.paths.get(path).map(|&c| c)
    }

    fn insert(&mut self, path: &str, method: Callback<T>) {
        println!("register callback for path: {}", path);
        self.paths.insert(path.to_owned(), method);
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

    pub fn get(&mut self, path: &str, method: Callback<T>) {
        self.router_get.insert(path, method);
    }

    pub fn post(&mut self, path: &str, method: Callback<T>) {
        self.router_post.insert(path, method);
    }

    pub fn put(&mut self, path: &str, method: Callback<T>) {
        self.router_put.insert(path, method);
    }

    pub fn delete(&mut self, path: &str, method: Callback<T>) {
        self.router_delete.insert(path, method);
    }

    pub fn head(&mut self, path: &str, method: Callback<T>) {
        self.router_head.insert(path, method);
    }

    pub fn start(self, addr: &str) -> Result<()> {
        Server::http(addr).unwrap().handle(self).unwrap();
        Ok(())
    }

    fn find_callback<'a, 'k>(&'a self, req: &'a HttpRequest<'a, 'k>) -> Option<Callback<T>> {
        use hyper::method::Method::*;

        let path = match req.uri {
            hyper::uri::RequestUri::AbsolutePath(ref path) => path,
            _ => return None
        };

        let router = match req.method {
            Get => &self.router_get,
            Post => &self.router_post,
            Put => &self.router_put,
            Delete => &self.router_delete,
            Head => &self.router_head,
            _ => { println!("unexpected method: {}", req.method); return None }
        };

        router.find(path)
    }
}

impl<T: 'static + Send + Sync> Handler for Container<T> {
    fn handle<'a, 'k>(&'a self, req: HttpRequest<'a, 'k>, mut res: HttpResponse<'a, Fresh>) {
        let mut req = Request::new(req);
        match self.find_callback(&req.inner) {
            None => *res.status_mut() = Status::NotFound,
            Some(f) => f(&self.inner, &mut req, Response::new(res)).unwrap()
        }

        // read the request body in case the callback did not read it
        // avoids a weird bug where Hyper does not correctly parse the method
        let mut buf = Vec::new();
        req.inner.read_to_end(&mut buf).unwrap();
    }
}
