extern crate hyper;

use hyper::header::Cookie as CookieHeader;
use hyper::header::{ContentLength, SetCookie};
pub use hyper::header::CookiePair as Cookie;

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

    pub fn cookies(&self) -> Vec<Cookie> {
        self.inner.headers.get::<CookieHeader>().map_or(Vec::new(),
            |&CookieHeader(ref cookies)| cookies.clone()
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

    pub fn send(self, content: &str) -> Result<()> {
        self.inner.send(content.as_bytes())
    }

    pub fn set_len(&mut self, len: u64) {
        self.inner.headers_mut().set(ContentLength(len))
    }

    pub fn set_type(&mut self, mime: String) {
        self.inner.headers_mut().set_raw("Content-Type", vec![mime.into_bytes()])
    }

    pub fn stream<F, R>(self, source: F) -> Result<()> where F: FnOnce(&mut Write) -> Result<R> {
        let mut streaming = try!(self.inner.start());
        try!(source(&mut streaming));
        streaming.end()
    }
}

pub type Callback<T> = fn(&T, &Request, Response) -> Result<()>;

pub struct Container<T: Send + Sync> {
    inner: T,
    routers: HashMap<String, Callback<T>>
}

impl<T: 'static + Send + Sync> Container<T> {

    pub fn new(inner: T) -> Container<T> {
        Container {
            inner: inner,
            routers: HashMap::new()
        }
    }

    pub fn get(&mut self, path: &str, method: Callback<T>) {
        println!("register callback for path: {}", path);
        self.routers.insert(path.to_owned(), method);
    }

    pub fn start(self, addr: &str) -> Result<()> {
        Server::http(addr).unwrap().handle(self).unwrap();
        Ok(())
    }
}

impl<T: Send + Sync> Handler for Container<T> {
    fn handle<'a, 'k>(&'a self, req: HttpRequest<'a, 'k>, res: HttpResponse<'a, Fresh>) {
        let method = self.routers["/"];
        method(&self.inner, &Request::new(req), Response::new(res)).unwrap();
    }
}
