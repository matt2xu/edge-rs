use hyper::header::{CookiePair as Cookie, ContentLength, ContentType, Header, HeaderFormat, Location, SetCookie};
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

use std::sync::Arc;
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};

use buffer::Buffer;

pub struct Resp {
    status: Option<Status>,
    headers: Option<Headers>,
    body: Buffer
}

impl Resp {
    fn new() -> Resp {
        Resp {
            status: None,
            body: Buffer::new(),
            headers: None
        }
    }

    pub fn body(&mut self) -> &mut Buffer { &mut self.body }

    pub fn deconstruct(&mut self) -> (Status, Headers) { (self.status.take().unwrap(), self.headers.take().unwrap()) }
}

pub struct Response {
    resp: Option<Resp>,
    ctrl: Control,
    tx: mpsc::Sender<Resp>,
    notify: Arc<AtomicBool>
}

impl Drop for Response {
    fn drop(&mut self) {
        self.tx.send(self.resp.take().unwrap()).unwrap();
        if self.notify.load(Ordering::Relaxed) {
            self.ctrl.ready(Next::write()).unwrap();
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

pub fn new(ctrl: Control, tx: mpsc::Sender<Resp>) -> Response {
    Response {
        resp: Some(Resp::new()),
        ctrl: ctrl,
        tx: tx,
        notify: Arc::new(AtomicBool::new(false))
    }
}

pub fn get_notify(response: &mut Response) -> Arc<AtomicBool> {
    response.notify.clone()
}

impl Response {
    fn resp(&mut self) -> &mut Resp {
        self.resp.as_mut().unwrap()
    }

    fn headers(&self) -> &Headers {
        self.resp.as_ref().unwrap().headers.as_ref().unwrap()
    }

    fn headers_mut(&mut self) -> &mut Headers {
        self.resp.as_mut().unwrap().headers.as_mut().unwrap()
    }

    /// Sets the status code of this response.
    pub fn status(&mut self, status: Status) -> &mut Self {
        self.resp().status = Some(status);
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
    pub fn header<H: Header + HeaderFormat>(&mut self, header: H) -> &mut Self {
        self.headers_mut().set(header);
        self
    }

    /// Sets the given header with raw strings.
    pub fn header_raw<K: Into<Cow<'static, str>> + Debug, V: Into<Vec<u8>>>(&mut self, name: K, value: V) -> &mut Self {
        self.headers_mut().set_raw(name, vec![value.into()]);
        self
    }

    /// Ends this response with the given status and an empty body
    pub fn end(mut self, status: Status) {
        self.status(status);
        self.len(0);
    }

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
        self.resp().body.send(content);
        let length = self.resp().body.len() as u64;
        self.len(length);
    }

    /// Sends the given content and ends this response.
    /// Status defaults to 200 Ok, headers must have been set before this method is called.
    pub fn append<D: AsRef<[u8]>>(&mut self, content: D) {
        self.resp().body.append(content.as_ref());
        let length = self.resp().body.len() as u64;
        self.len(length);
    }

    /// Sends the given file, setting the Content-Type based on the file's extension.
    /// Known extensions are htm, html, jpg, jpeg, png, js, css.
    /// If the file does not exist, this method sends a 404 Not Found response.
    pub fn send_file<P: AsRef<Path>>(mut self, path: P) {
        if !self.headers().has::<ContentType>() {
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
                    self.headers_mut().set(content_type);
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

        if self.headers().has::<SetCookie>() {
            self.headers_mut().get_mut::<SetCookie>().unwrap().push(cookie)
        } else {
            self.headers_mut().set(SetCookie(vec![cookie]))
        }
    }
}
