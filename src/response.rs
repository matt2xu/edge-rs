use hyper::header::{self, CookiePair as Cookie, ContentType, Header, SetCookie};
use hyper::status::StatusCode as Status;

use hyper::{Control, Headers, Next};

use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};

use handlebars::Handlebars;
use serde::ser::Serialize as ToJson;

pub use serde_json::value as value;

use std::borrow::Cow;
use std::cell::UnsafeCell;
use std::fmt::Debug;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::Path;
use std::sync::{Arc};

use buffer::Buffer;
use crossbeam::sync::chase_lev::{deque, Steal, Stealer, Worker};

pub struct Resp {
    status: Status,
    headers: Headers,

    body: Buffer,
    worker: Option<Worker<Buffer>>,
    stealer: Option<Stealer<Buffer>>,

    handlebars: &'static Handlebars,
    ctrl: Control
}

impl Resp {
    pub fn new(handlebars: &'static Handlebars, ctrl: Control) -> Resp {
        Resp {
            status: Status::Ok,
            headers: Headers::default(),
            body: Buffer::new(),
            worker: None,
            stealer: None,

            handlebars: handlebars,
            ctrl: ctrl
        }
    }

    /// notify handler we have something to write
    /// called by drop and append
    fn notify(&self) {
        if let Err(e) = self.ctrl.ready(Next::write()) {
            error!("could not notify handler: {}", e);
        }
    }

    fn is_streaming(&self) -> bool {
        self.worker.is_some()
    }

    fn status(&mut self, status: Status) {
        self.status = status;
    }

    fn has_header<H: Header>(&self) -> bool {
        self.headers.has::<H>()
    }

    fn header<H: Header>(&mut self, header: H) {
        self.headers.set(header);
    }

    fn header_raw<K: Into<Cow<'static, str>> + Debug>(&mut self, name: K, value: Vec<Vec<u8>>) {
        self.headers.set_raw(name, value);
    }

    fn push_cookie(&mut self, cookie: Cookie) {
        self.headers.get_mut::<SetCookie>().unwrap().push(cookie)
    }

    fn len(&self) -> usize {
        self.body.len()
    }

    fn append<D: Into<Buffer>>(&mut self, buffer: D) {
        self.worker.as_mut().unwrap().push(buffer.into());
        self.notify();
    }

    fn send<D: Into<Vec<u8>>>(&mut self, content: D) {
        self.body.send(content);
    }

    fn init_deque(&mut self) {
        let (worker, stealer) = deque();
        self.worker = Some(worker);
        self.stealer = Some(stealer);
    }
}

/// This holds data for the response.
pub struct ResponseHolder {
    resp: Arc<UnsafeCell<Resp>>
}

impl ResponseHolder {

    pub fn new(handlebars: &'static Handlebars, control: Control) -> ResponseHolder {
        ResponseHolder {
            resp: Arc::new(UnsafeCell::new(Resp::new(handlebars, control)))
        }
    }

    pub fn new_response(&mut self) -> Response {
        Response {
            resp: self.resp.clone()
        }
    }

    fn resp(&self) -> &Resp {
        unsafe { &*self.resp.get() }
    }

    pub fn get_status(&self) -> Status {
        self.resp().status
    }

    pub fn set_headers(&self, headers: &mut Headers) {
        *headers = self.resp().headers.clone();
    }

    pub fn is_streaming(&self) -> bool {
        self.resp().is_streaming()
    }

    pub fn body(&mut self) -> &mut Buffer {
        unsafe { &mut (*self.resp.get()).body }
    }

    pub fn pop(&mut self) -> Option<Buffer> {
        match self.resp().stealer.as_ref().unwrap().steal() {
            Steal::Data(buffer) => Some(buffer),
            Steal::Empty => None,
            Steal::Abort => panic!("abort")
        }
    }
}

/// This represents the response that will be sent back to the application.
///
/// Includes a status code (default 200 OK), headers, and a body.
/// The response can be updated and sent back immediately in a synchronous way,
/// or deferred pending some computation (asynchronous mode).
///
/// The response is sent when it is dropped.
pub struct Response {
    resp: Arc<UnsafeCell<Resp>>
}

// no worries, the response is always modified by only one thread at a time
unsafe impl Send for Response {}

impl Drop for Response {
    fn drop(&mut self) {
        self.resp().notify();
    }
}

impl Response {

    fn resp(&self) -> &Resp {
        unsafe {
            &*self.resp.get()
        }
    }

    fn resp_mut(&self) -> &mut Resp {
        unsafe {
            &mut *self.resp.get()
        }
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.resp_mut().status(status);
        self
    }

    /// Sets the Content-Type header.
    pub fn content_type<S: Into<Vec<u8>>>(&mut self, mime: S) -> &mut Self {
        self.header_raw("Content-Type", mime)
    }

    /// Sets the Content-Length header.
    pub fn len(&mut self, len: u64) -> &mut Self {
        self.header(header::ContentLength(len))
    }

    /// Sets the given cookie.
    pub fn cookie(&mut self, cookie: Cookie) {
        let has_cookie_header = self.resp().has_header::<SetCookie>();
        if has_cookie_header {
            self.resp_mut().push_cookie(cookie)
        } else {
            self.resp_mut().header(SetCookie(vec![cookie]))
        }
    }

    /// Sets the given header.
    pub fn header<H: Header>(&mut self, header: H) -> &mut Self {
        self.resp_mut().header(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.resp_mut().header_raw(name, vec![value.into()]);
        self
    }

    /// Sets the Location header.
    pub fn location<S: Into<String>>(&mut self, url: S) -> &mut Self {
        self.header(header::Location(url.into()))
    }

    /// Redirects to the given URL with the given status, or 302 Found if none is given.
    pub fn redirect(mut self, url: &str, status: Option<Status>) {
        self.location(url);
        self.end(status.unwrap_or(Status::Found))
    }

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) {
        self.status(status);
        self.len(0);
    }

    /// Renders the template with the given name using the given data.
    ///
    /// If no Content-Type header is set, the content type is set to `text/html`.
    pub fn render<T: ToJson>(self, name: &str, data: T) {
        let need_content_type = !self.resp().has_header::<ContentType>();
        if need_content_type {
            self.resp_mut().header(ContentType::html());
        }

        let result = self.resp().handlebars.render(name, &data);
        self.send(result.unwrap())
    }

    /// Sends the given content and ends this response.
    ///
    /// Status defaults to 200 Ok, headers must have been set before this method is called.
    pub fn send<D: Into<Vec<u8>>>(mut self, content: D) {
        self.resp_mut().send(content);
        let length = self.resp().len();
        self.len(length as u64);
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    ///
    /// Known extensions are:
    ///   - application: js, m3u8, mpd, xml
    ///   - image: gif, jpg, jpeg, png
    ///   - text: css, htm, html, txt
    ///   - video: avi, mp4, mpg, mpeg, ts
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(mut self, path: P) {
        let need_content_type = !self.resp().has_header::<ContentType>();
        if need_content_type {
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
                    self.resp_mut().header(ContentType(Mime(TopLevel::Ext(top.to_string()),
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

    /// Moves to streaming mode.
    ///
    /// If no Content-Length is set, use Transfer-Encoding: chunked
    pub fn stream(self) -> Streaming {
        self.resp_mut().init_deque();
        Streaming {
            resp: self.resp.clone()
        }
    }
}

pub struct Streaming {
    resp: Arc<UnsafeCell<Resp>>
}

// no worries, the response is always modified by only one thread at a time
unsafe impl Send for Streaming {}

impl Drop for Streaming {
    fn drop(&mut self) {
        // append an empty buffer to indicate there is no more data left, and notify handler
        self.resp_mut().append(vec![]);
    }
}

impl Streaming {

    fn resp_mut(&self) -> &mut Resp {
        unsafe {
            &mut *self.resp.get()
        }
    }

    /// Appends the given content to this response's body.
    pub fn append<D: Into<Vec<u8>>>(&mut self, content: D) {
        let vec = content.into();
        debug!("append {} bytes", vec.len());
        self.resp_mut().append(vec);
    }
}
