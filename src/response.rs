use hyper::header::{self, CookiePair as Cookie, ContentType, Header, SetCookie};
use hyper::status::StatusCode as Status;

use hyper::Headers;
use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};

use serde_json::value as json;
use serde_json::value::ToJson;

use std::any::Any;
use std::boxed::Box;
use std::borrow::Cow;
use std::{error, fmt, result};
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::path::Path;

/// Defines a handler error
#[derive(Debug)]
pub struct Error {
    pub status: Status,
    pub message: Option<Cow<'static, str>>
}

pub type Result = result::Result<Action, Error>;

impl Error {
    fn new(status: Status, message: Option<Cow<'static, str>>) -> Error {
        Error {
            status: status,
            message: message
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use std::error::Error;
        self.description().fmt(f)
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match self.message {
            None => "<no description available>",
            Some(ref message) => &message
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        None
    }
}

impl From<Status> for Error {
    fn from(status: Status) -> Error {
        Error::new(status, None)
    }
}

impl From<(Status, &'static str)> for Error {
    fn from(pair: (Status, &'static str)) -> Error {
        Error::new(pair.0, Some(Cow::Borrowed(pair.1)))
    }
}

impl From<(Status, String)> for Error {
    fn from(pair: (Status, String)) -> Error {
        Error::new(pair.0, Some(Cow::Owned(pair.1)))
    }
}

/// Defines the action to be taken when returning from a handler
pub enum Action {
    /// Ends the response with no body and the given status (if given).
    ///
    /// If the status is not given, the status currently set on the response is used.
    /// By default, a response has a status 200 OK.
    End(Option<Status>),

    /// Redirects to the given URL with a 3xx status (use 302 Found if unsure).
    Redirect(Status, String),

    /// Renders the template with the given name using the given JSON value.
    ///
    /// If no Content-Type header is set, the content type is set to `text/html`.
    Render(String, json::Value),

    /// Sends the response with the given bytes as the body.
    Send(Vec<u8>),

    /// Returns a closure that is called with a Stream argument.
    Stream(Box<Fn(&mut Any, &mut Write)>),

    /// Sends the given file, setting the Content-Type based on the file's extension.
    ///
    /// Known extensions are:
    ///   - application: js, m3u8, mpd, xml
    ///   - image: gif, jpg, jpeg, png
    ///   - text: css, htm, html, txt
    ///   - video: avi, mp4, mpg, mpeg, ts
    /// If the file does not exist, this method sends a 404 Not Found response.
    SendFile(String)
}

/// Conversion from `()` into `End(None)`.
impl From<()> for Action {
    fn from(_: ()) -> Action {
        Action::End(None)
    }
}

/// Conversion from `Status` into `End(Some(status))`.
impl From<Status> for Action {
    fn from(status: Status) -> Action {
        Action::End(Some(status))
    }
}

/// Conversion from `(Status, &str)` into `Action::Redirect(status, url)`.
impl<'a> From<(Status, &'a str)> for Action {
    fn from(pair: (Status, &'a str)) -> Action {
        Action::Redirect(pair.0, pair.1.to_string())
    }
}

/// Conversion from `(Status, String)` into `Action::Redirect(status, url)`.
impl From<(Status, String)> for Action {
    fn from(pair: (Status, String)) -> Action {
        From::from((pair.0, pair.1.as_str()))
    }
}

/// Conversion from `(&str, T)`, where `T` can be converted to a JSON value,
/// into `Action::Render(template_name, json)`.
impl<'a, T> From<(&'a str, T)> for Action where T: ToJson {
    fn from(pair: (&'a str, T)) -> Action {
        Action::Render(pair.0.to_string(), pair.1.to_json())
    }
}

/// Conversion from `(String, T)`, where `T` can be converted to a JSON value,
/// into `Action::Render(template_name, json)`.
impl<T> From<(String, T)> for Action where T: ToJson {
    fn from(pair: (String, T)) -> Action {
        Action::Render(pair.0, pair.1.to_json())
    }
}

/// Conversion from `Vec<u8>` into `Action::Send(bytes)`.
impl From<Vec<u8>> for Action {
    fn from(bytes: Vec<u8>) -> Action {
        Action::Send(bytes)
    }
}

/// Conversion from `&str` into `Action::Send(bytes)`.
impl<'a> From<&'a str> for Action {
    fn from(string: &'a str) -> Action {
        Action::Send(string.as_bytes().to_vec())
    }
}

/// Conversion from `String` into `Action::Send(bytes)`.
impl From<String> for Action {
    fn from(string: String) -> Action {
        Action::Send(string.into_bytes())
    }
}

/// Conversion from `json::Value` into `Action::Send(bytes)`.
impl From<json::Value> for Action {
    fn from(json: json::Value) -> Action {
        From::from(json.to_string())
    }
}

/// Wraps the given closure in a box and returns `Ok(Action::Stream(box))`.
///
/// The closure will be called with a writer implementing the `Write` trait
/// so that each call to `write` notifies the handler that data can be written
/// to the HTTP transport.
pub fn stream<F, T, R>(closure: F) -> Result where T: Any, F: 'static + Fn(&mut T, &mut Write) -> io::Result<R> {
    Ok(Action::Stream(Box::new(move |any, writer| {
        if let Some(app) = any.downcast_mut::<T>() {
            if let Err(e) = closure(app, writer) {
                error!("{}", e);
            }
        }
    })))
}

/// This represents the response that will be sent back to the application.
///
/// Includes a status code (default 200 OK), headers, and a body.
/// The response can be updated and sent back immediately in a synchronous way,
/// or deferred pending some computation (asynchronous mode).
///
/// The response is sent when it is dropped.
pub struct Response {
    pub status: Status,
    pub headers: Headers,
    streaming: bool
}

impl Response {

    pub fn new() -> Response {
        Response {
            status: Status::Ok,
            headers: Headers::default(),
            streaming: false
        }
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.status = status;
        self
    }

    /// Sets the Content-Type header.
    pub fn content_type<S: Into<Vec<u8>>>(&mut self, mime: S) -> &mut Self {
        self.headers.set_raw("Content-Type", vec![mime.into()]);
        self
    }

    /// Sets the Content-Length header.
    pub fn len(&mut self, len: u64) -> &mut Self {
        self.headers.set(header::ContentLength(len));
        self
    }

    /// Sets the given cookie.
    pub fn cookie(&mut self, cookie: Cookie) {
        if self.headers.has::<SetCookie>() {
            self.headers.get_mut::<SetCookie>().unwrap().push(cookie)
        } else {
            self.headers.set(SetCookie(vec![cookie]))
        }
    }

    /// Sets the given header.
    pub fn header<H: Header>(&mut self, header: H) -> &mut Self {
        self.headers.set(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + fmt::Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.headers.set_raw(name, vec![value.into()]);
        self
    }

    /// Sets the Location header.
    pub fn location<S: Into<String>>(&mut self, url: S) -> &mut Self {
        self.headers.set(header::Location(url.into()));
        self
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    ///
    /// Known extensions are:
    ///   - application: js, m3u8, mpd, xml
    ///   - image: gif, jpg, jpeg, png
    ///   - text: css, htm, html, txt
    ///   - video: avi, mp4, mpg, mpeg, ts
    /// If the file does not exist, this method sends a 404 Not Found response.
    fn send_file<P: AsRef<Path>>(&mut self, path: P) -> Option<Vec<u8>> {
        if !self.headers.has::<ContentType>() {
            let extension = path.as_ref().extension();
            if let Some(ext) = extension {
                let content_type = match ext.to_string_lossy().as_ref() {
                    // application
                    "js" => Some(("application", "javascript", None)),
                    "m3u8" => Some(("application", "vnd.apple.mpegurl", None)),
                    "mpd" => Some(("application", "dash+xml", None)),
                    "xml" => Some(("application", "xml", None)),

                    // image
                    "gif" => Some(("image", "gif", None)),
                    "jpg" | "jpeg" => Some(("image", "jpeg", None)),
                    "png" => Some(("image", "png", None)),

                    // text
                    "css" => Some(("text", "css", None)),
                    "htm" | "html" => Some(("text", "html", Some((Attr::Charset, Value::Utf8)))),
                    "txt" => Some(("text", "plain", Some((Attr::Charset, Value::Utf8)))),

                    // video
                    "avi" => Some(("video", "x-msvideo", None)),
                    "mp4" => Some(("video", "mp4", None)),
                    "mpg" | "mpeg" => Some(("video", "mpeg", None)),
                    "ts" => Some(("video", "mp2t", None)),
                    _ => None
                };

                if let Some((top, sub, attr)) = content_type {
                    self.headers.set(ContentType(Mime(TopLevel::Ext(top.to_string()),
                        SubLevel::Ext(sub.to_string()),
                        match attr {
                            None => vec![],
                            Some(val) => vec![val]
                        }
                    )));
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
                    Some(format!("{}", err).into())
                } else {
                    Some(buf)
                }
            },
            Err(ref err) if err.kind() == ErrorKind::NotFound => {
                self.status(Status::NotFound);
                None
            },
            Err(ref err) => {
                self.status(Status::InternalServerError).content_type("text/plain");
                Some(format!("{}", err).into())
            }
        }
    }

}

pub fn send_file<P: AsRef<Path>>(response: &mut Response, path: P) -> Option<Vec<u8>> {
    response.send_file(path)
}

pub fn set_streaming(response: &mut Response) {
    response.streaming = true;
}

pub fn is_streaming(response: &Response) -> bool {
    response.streaming
}
