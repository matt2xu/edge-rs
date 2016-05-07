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
    status: RefCell<Status>,
    headers: RefCell<Headers>,
    body: RefCell<Buffer>,

    notify: AtomicBool,
    ctrl: Control
}

// no worries, the resp is always modified by only one thread at a time
unsafe impl Sync for Resp {}

impl Resp {
    pub fn new(ctrl: Control) -> Resp {
        Resp {
            status: RefCell::new(Status::Ok),
            headers: RefCell::new(Headers::default()),
            body: RefCell::new(Buffer::new()),

            notify: AtomicBool::new(false),
            ctrl: ctrl
        }
    }

    fn status(&self, status: Status) {
        *self.status.borrow_mut() = status;
    }

    fn has_header<H: Header>(&self) -> bool {
        self.headers.borrow().has::<H>()
    }

    fn header<H: Header>(&self, header: H) {
        self.headers.borrow_mut().set(header);
    }

    fn header_raw<K: Into<Cow<'static, str>> + Debug>(&self, name: K, value: Vec<Vec<u8>>) {
        self.headers.borrow_mut().set_raw(name, value);
    }

    fn push_cookie(&self, cookie: Cookie) {
        self.headers.borrow_mut().get_mut::<SetCookie>().unwrap().push(cookie)
    }

    fn len(&self) -> usize {
        self.body.borrow().len()
    }

    fn append<D: AsRef<[u8]>>(&self, content: D) {
        self.body.borrow_mut().append(content.as_ref());
    }

    fn send<D: Into<Vec<u8>>>(&self, content: D) {
        self.body.borrow_mut().send(content);
    }

    pub fn deconstruct(self) -> (Status, Headers, Buffer) {
        (self.status.into_inner(),
        self.headers.into_inner(),
        self.body.into_inner())
    }

    fn done(&self) {
        if self.notify.load(Ordering::Acquire) {
            self.ctrl.ready(Next::write()).unwrap();
        }
    }
}

pub fn set_notify(resp: &Option<Arc<Resp>>) {
    if let Some(ref arc) = *resp {
        arc.notify.store(true, Ordering::Release);   
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
    resp: Option<Arc<Resp>>
}

impl Drop for Response {
    fn drop(&mut self) {
        // this is to make sure that we remove this Response's strong reference to Resp
        // *before* we notify the handler, so the call to Arc::get_mut succeeds
        let resp = self.resp.as_ref().unwrap().as_ref() as *const Resp;

        // drop Arc
        {
            self.resp.take().unwrap();
        }

        // no worries: Resp is not dropped when the Arc is dropped,
        // because the handler outlives us, therefore the pointer is always valid here.
        unsafe {
            (*resp).done();
        }
    }
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

pub fn new(resp: &Option<Arc<Resp>>) -> Response {
    Response {
        resp: Some(resp.as_ref().unwrap().clone())
    }
}

impl Response {

    fn resp(&self) -> &Resp {
        &self.resp.as_ref().unwrap()
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.resp().status(status);
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
        self.resp().header(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.resp().header_raw(name, vec![value.into()]);
        self
    }

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) {
        self.status(status);
        self.len(0);
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
        self.resp().send(content);
        let length = self.resp().len();
        self.len(length as u64);
    }

    /// Sends the given content and ends this response.
    /// Status defaults to 200 Ok, headers must have been set before this method is called.
    pub fn append<D: AsRef<[u8]>>(&mut self, content: D) {
        self.resp().append(content);
        let length = self.resp().len() as u64;
        self.len(length);
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    /// Known extensions are htm, html, jpg, jpeg, png, js, css.
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(mut self, path: P) {
        if !self.resp().has_header::<ContentType>() {
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
                    self.resp().header(content_type);
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

        if self.resp().has_header::<SetCookie>() {
            self.resp().push_cookie(cookie)
        } else {
            self.resp().header(SetCookie(vec![cookie]))
        }
    }
}
