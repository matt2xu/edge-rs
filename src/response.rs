use hyper::header::{CookiePair as Cookie, ContentLength, ContentType, Header, Location, SetCookie};
use hyper::status::StatusCode as Status;

use hyper::{Control, Headers, Next};

use hyper::mime::{Mime, TopLevel, SubLevel, Attr, Value};

use handlebars::Handlebars;
use serde::ser::Serialize as ToJson;

pub use serde_json::value as value;

use std::fmt::Debug;
use std::borrow::Cow;
use std::io::{ErrorKind, Read, Result, Write};
use std::fs::{File, read_dir};
use std::path::Path;

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use buffer::Buffer;

#[derive(Debug)]
pub struct Resp {
    status: Status,
    headers: Headers,
    body: Buffer
}

impl Resp {
    pub fn new() -> Resp {
        Resp {
            status: Status::Ok,
            headers: Headers::default(),
            body: Buffer::new()
        }
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

    fn append<D: AsRef<[u8]>>(&mut self, content: D) {
        self.body.append(content.as_ref());
    }

    fn send<D: Into<Vec<u8>>>(&mut self, content: D) {
        self.body.send(content);
    }

    pub fn deconstruct(self) -> (Status, Headers, Buffer) {
        (self.status, self.headers, self.body)
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
    ended_or_notify: Arc<AtomicBool>,
    ctrl: Arc<Control>,
    resp: Option<Arc<RefCell<Resp>>>
}

// no worries, the response is always modified by only one thread at a time
unsafe impl Send for Response {}
unsafe impl Sync for Response {}

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

pub fn new(control: Control) -> Response {
    Response {
        ended_or_notify: Arc::new(AtomicBool::new(false)),
        ctrl: Arc::new(control),
        resp: Some(Arc::new(RefCell::new(Resp::new())))
    }
}

pub fn clone(response: &Response) -> Response {
    Response {
        ended_or_notify: response.ended_or_notify.clone(),
        ctrl: response.ctrl.clone(),
        resp: Some(response.resp.as_ref().unwrap().clone())
    }
}

/// Deconstructs the given response.
pub fn deconstruct(response: &mut Response) -> (Status, Headers, Buffer) {
    let resp = Arc::try_unwrap(response.resp.take().unwrap()).unwrap().into_inner();
    resp.deconstruct()
}

/// two possible cases:
///   - done before can_write: in synchronous style, done is called first,
///     sets ended_or_notify to true and does not notify the handler
///   - can_write before done: response not done yet, can_write is called first,
///     sets ended_or_notify to true so that when done is called, it will notify the handler
pub fn can_write(response: &mut Response) -> bool {
    if response.ended_or_notify.compare_and_swap(false, true, Ordering::AcqRel) {
        // if true, response already ended, we can write it
        true
    } else {
        // if false, response did not end yet
        // ended has been set to true to mean "need to notify handler"
        false
    }
}

impl Response {

    fn resp(&self) -> &RefCell<Resp> {
        &self.resp.as_ref().unwrap()
    }

    fn done(&mut self) {
        if self.ended_or_notify.compare_and_swap(false, true, Ordering::AcqRel) {
            // if true, means we need to notify, the flag is not updated

            // drop Arc to make sure that we remove this Response's strong reference to Resp
            // *before* we notify the handler, so the call to Arc::try_unwrap succeeds
            let _ = self.resp.take().unwrap();
            self.ctrl.ready(Next::write()).unwrap();
        } else {
            // if previously false: no need to notify
            // ended has been set to true to mean "response ended"
        }
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.resp().borrow_mut().status(status);
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
    pub fn header<H: Header>(&mut self, header: H) -> &mut Self {
        self.resp().borrow_mut().header(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.resp().borrow_mut().header_raw(name, vec![value.into()]);
        self
    }

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) {
        self.status(status);
        self.len(0);
        self.done();
    }

    /// Renders the template at the given path using the given data.
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
        self.resp().borrow_mut().send(content);
        let length = self.resp().borrow().len();
        self.len(length as u64);
        self.done();
    }

    /// Appends the given content to this response's body.
    /// Will change to support asynchronous use case.
    pub fn append<D: AsRef<[u8]>>(&mut self, content: D) {
        self.resp().borrow_mut().append(content);
        let length = self.resp().borrow().len() as u64;
        self.len(length);
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    /// Known extensions are htm, html, jpg, jpeg, png, js, css.
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(mut self, path: P) {
        let need_content_type = !self.resp().borrow().has_header::<ContentType>();
        if need_content_type {
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
                    self.resp().borrow_mut().header(content_type);
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

    /*
    /// Writes the body of this response using the given source function.
    pub fn stream<F, R>(&mut self, source: F) -> Result<()> where F: FnOnce(&mut Write) -> Result<R> {
        //let mut streaming = try!(self.inner.start());
        //try!(source(&mut streaming));
        //streaming.end()
        Ok(())
    }
    */

    /// Sets the given cookie.
    pub fn cookie(&mut self, cookie: Cookie) {
        let resp = self.resp();
        let has_cookie_header = resp.borrow().has_header::<SetCookie>();
        if has_cookie_header {
            resp.borrow_mut().push_cookie(cookie)
        } else {
            resp.borrow_mut().header(SetCookie(vec![cookie]))
        }
    }
}
