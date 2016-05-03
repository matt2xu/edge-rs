extern crate url;

pub use hyper::header as header;
use header::{Cookie as CookieHeader, ContentLength, ContentType};
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::{Headers, Method, Post};

use hyper::method::Method::{Put};
use hyper::uri::RequestUri::{AbsolutePath, Star};

use hyper::server::Request as HttpRequest;

use hyper::mime::{Mime, TopLevel, SubLevel};

use std::collections::BTreeMap;
use std::io::{Error, ErrorKind, Result};
use std::slice::Iter;

use buffer::Buffer;

/// A request, with a path, query, and fragment (accessor methods not yet implemented for the last two).
///
/// Can be queried for the parameters that were matched by the router.
pub struct Request {
    inner: HttpRequest,
    path: Vec<String>,
    query: Option<String>,
    fragment: Option<String>,

    params: Option<BTreeMap<String, String>>,
    body: Option<Buffer>
}

pub fn new(inner: HttpRequest) -> url::ParseResult<Request> {
    let (path, query, fragment) = match *inner.uri() {
        AbsolutePath(ref path) => match url::parse_path(path) {
            Ok(res) => res,
            Err(e) => return Err(e)
        },
        Star => (vec!["*".to_owned()], None, None),
        _ => panic!("unsupported request URI")
    };

    let body = match *inner.method() {
        Put | Post => Some(match inner.headers().get::<ContentLength>() {
            Some(&ContentLength(len)) => Buffer::with_capacity(len as usize),
            None => Buffer::new()
        }),
        _ => None
    };

    Ok(Request {
        inner: inner,
        path: path,
        query: query,
        fragment: fragment,
        params: None,
        body: body})
}

pub fn body(request: &mut Request) -> &mut Buffer {
    request.body.as_mut().unwrap()
}

impl Request {
    /// Reads this request's body until the end, and returns it as a vector of bytes.
    pub fn body(&self) -> Result<&[u8]> {
        match self.body {
            Some(ref buffer) => Ok(buffer.as_ref()),
            None => Err(Error::new(ErrorKind::UnexpectedEof, "empty body"))
        }
    }

    /// Returns an iterator over the cookies of this request.
    pub fn cookies(&self) -> Iter<Cookie> {
        self.headers().get::<CookieHeader>().map_or([].iter(),
            |&CookieHeader(ref cookies)| cookies.iter()
        )
    }

    /// Reads the body of this request, parses it as an application/x-www-form-urlencoded format,
    /// and returns it as a vector of (name, value) pairs.
    pub fn form(&mut self) -> Result<Vec<(String, String)>> {
        let body = try!(self.body());

        match self.headers().get::<ContentType>() {
            Some(&ContentType(Mime(TopLevel::Application, SubLevel::WwwFormUrlEncoded, _))) =>
                Ok(url::form_urlencoded::parse(body)),
            Some(_) => Err(Error::new(ErrorKind::InvalidInput, "invalid Content-Type, expected application/x-www-form-urlencoded")),
            None => Err(Error::new(ErrorKind::InvalidInput, "missing Content-Type header"))
        }
    }

    /// Returns the method
    pub fn method(&self) -> &Method {
        self.inner.method()
    }

    /// Returns headers
    #[inline]
    pub fn headers(&self) -> &Headers { self.inner.headers() }

    /// Returns the parameter with the given name declared by the route that matched the URL of this request (if any).
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.as_ref().map_or(None, |map| map.get(key).map(String::as_str))
    }

    /// Returns the path of this request, i.e. the list of segments of the URL.
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// Returns the query of this request (if any).
    pub fn query(&self) -> Option<&str> {
        self.query.as_ref().map(String::as_str)
    }

    /// Returns the fragment of this request (if any).
    pub fn fragment(&self) -> Option<&str> {
        self.fragment.as_ref().map(String::as_str)
    }

    /// Sets the parameters declared by the route that matched the URL of this request.
    pub fn set_params(&mut self, params: BTreeMap<String, String>) {
        self.params = Some(params);
    }
}
