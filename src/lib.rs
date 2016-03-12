extern crate hyper;

use hyper::header::Cookie as CookieHeader;
use hyper::header::{ContentLength, SetCookie};
pub use hyper::header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::net::Fresh;
use hyper::server::{Handler, Server};
use hyper::server::Request as HttpRequest;
use hyper::server::Response as HttpResponse;

use std::collections::HashMap;
use std::io::{Result, Write};

pub struct Request<'a, 'b: 'a> {
    inner: HttpRequest<'a, 'b>
}

impl<'a, 'b> Request<'a, 'b> {
    fn new(inner: HttpRequest<'a, 'b>) -> Request<'a, 'b> {
        Request {
            inner: inner
        }
    }

    pub fn cookies(&self) -> std::slice::Iter<Cookie> {
        self.inner.headers.get::<CookieHeader>().map_or([].iter(),
            |&CookieHeader(ref cookies)| cookies.iter()
        )
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
    
    pub fn cookie<F>(&mut self, name: &str, value: &str, set_options: Option<F>) where F: Fn(&mut Cookie) {
        let mut cookie = Cookie::new(name.to_owned(), value.to_owned());
        set_options.map(|f| f(&mut cookie));

        if self.inner.headers().has::<SetCookie>() {
            self.inner.headers_mut().get_mut::<SetCookie>().unwrap().push(cookie)
        } else {
            self.inner.headers_mut().set(SetCookie(vec![cookie]))
        }
    }

    pub fn send<D: AsRef<[u8]>>(self, content: D) -> Result<()> {
        self.inner.send(content.as_ref())
    }

    pub fn set_len(&mut self, len: u64) {
        self.inner.headers_mut().set(ContentLength(len))
    }

    pub fn set_status(&mut self, status: Status) {
        *self.inner.status_mut() = status
    }

    pub fn set_type<S: Into<Vec<u8>>>(&mut self, mime: S) {
        self.inner.headers_mut().set_raw("Content-Type", vec![mime.into()])
    }

    pub fn stream<F, R>(self, source: F) -> Result<()> where F: FnOnce(&mut Write) -> Result<R> {
        let mut streaming = try!(self.inner.start());
        try!(source(&mut streaming));
        streaming.end()
    }
}

pub type Callback<T> = fn(&T, &Request, Response) -> Result<()>;

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
        match self.find_callback(&req) {
            None => *res.status_mut() = Status::NotFound,
            Some(f) => f(&self.inner, &Request::new(req), Response::new(res)).unwrap()
        }
    }
}
