extern crate url;

pub use hyper::header as header;
use header::{Cookie as CookieHeader, ContentType};
pub use header::CookiePair as Cookie;
pub use hyper::status::StatusCode as Status;

use hyper::{Headers, HttpVersion, Method};
use hyper::uri::RequestUri::{AbsolutePath, Star};
use hyper::mime::{Mime, TopLevel, SubLevel};
use hyper::server::Request as HttpRequest;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::{Error as IoError, ErrorKind};

use buffer::Buffer;

use serde_json as json;

use url::{ParseError, Url};

/// A request, with a path, query, and fragment (accessor methods not yet implemented for the last two).
///
/// Can be queried for the parameters that were matched by the router.
pub struct Request {
    inner: HttpRequest,
    url: Option<Url>,
    path: Vec<String>,
    query: Option<BTreeMap<String, String>>,
    params: Option<BTreeMap<String, String>>,
    body: Option<Buffer>
}

pub fn new(base_url: &Url, inner: HttpRequest) -> Result<Request, ParseError> {
    let url = match *inner.uri() {
        AbsolutePath(ref path) => Some(try!(base_url.join(path))),
        Star => None,
        _ => panic!("unsupported request URI")
    };

    let path = match url {
        None => vec!["*".to_owned()],
        Some(ref url) => url.path_segments().unwrap().map(|s| s.to_string()).collect()
    };

    let query = match url {
        None => None,
        Some(ref url) => Some(url.query_pairs().into_owned().collect())
    };

    Ok(Request {
        inner: inner,
        url: url,
        path: path,
        query: query,
        params: None,
        body: None})
}

pub fn set_body(request: Option<&mut Request>, body: Option<Buffer>) {
    if let Some(req) = request {
        req.body = body;
    }
}

impl Request {
    /// Returns this request's body as a vector of bytes.
    pub fn body(&self) -> Result<&[u8], IoError> {
        match self.body {
            Some(ref buffer) => Ok(buffer.as_ref()),
            None => Err(IoError::new(ErrorKind::UnexpectedEof, "empty body"))
        }
    }

    /// Returns an iterator over the cookies of this request.
    pub fn cookies(&self) -> ::std::slice::Iter<Cookie> {
        self.headers().get::<CookieHeader>().map_or([].iter(),
            |&CookieHeader(ref cookies)| cookies.iter()
        )
    }

    /// Parses the body of this request as an URL-encoded form.
    ///
    /// The Content-Type header must indicate ```application/x-www-form-urlencoded```.
    /// Returns a (key, value) map of clone-on-write strings.
    pub fn form<'a>(&'a self) -> Result<BTreeMap<Cow<'a, str>, Cow<'a, str>>, IoError> {
        let body = try!(self.body());

        match self.headers().get::<ContentType>() {
            Some(&ContentType(Mime(TopLevel::Application, SubLevel::WwwFormUrlEncoded, _))) => {
                let parse = url::form_urlencoded::parse(body);
                Ok(parse.collect())
            }
            Some(_) => Err(IoError::new(ErrorKind::InvalidInput, "invalid Content-Type, expected application/x-www-form-urlencoded")),
            None => Err(IoError::new(ErrorKind::InvalidInput, "missing Content-Type header"))
        }
    }

    /// Parses the body of this request as JSON (indicated by ```application/json``` content type).
    pub fn json(&self) -> Result<json::Value, json::Error> {
        let body = try!(self.body());

        match self.headers().get::<ContentType>() {
            Some(&ContentType(Mime(TopLevel::Application, SubLevel::Json, _))) => {
                json::from_slice(body)
            }
            Some(_) => Err(json::Error::Io(IoError::new(ErrorKind::InvalidInput, "invalid Content-Type, expected application/json"))),
            None => Err(json::Error::Io(IoError::new(ErrorKind::InvalidInput, "missing Content-Type header")))
        }
    }

    /// Returns the HTTP version
    pub fn version(&self) -> &HttpVersion {
        self.inner.version()
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

    /// Returns the parameter with the given name in this request's query (if any).
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query.as_ref().map_or(None, |map| map.get(key).map(String::as_str))
    }

    /// Returns the fragment of this request (if any).
    pub fn fragment(&self) -> Option<&str> {
        match self.url {
            None => None,
            Some(ref url) => url.fragment()
        }
    }
}

/// Sets the parameters declared by the route that matched the URL of this request.
pub fn set_params(request: &mut Request, params: BTreeMap<String, String>) {
    request.params = Some(params);
}
