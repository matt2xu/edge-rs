//! Router module

use hyper::Method;
use hyper::method::Method::{Delete, Get, Head, Post, Put};

use std::any::Any;
use std::collections::{BTreeMap, HashMap};
use std::marker::PhantomData;

use request;
use request::Request;
use response::Response;

pub type TypedCallback<T> = fn(&mut T, &Request, Response);
pub type TypedMiddleware<T> = fn(&mut T, &mut Request);
pub type Static = fn(&Request, Response);

/// A segment is either a fixed string, or a variable with a name
#[derive(Debug)]
enum Segment {
    Fixed(String),
    Variable(String)
}

impl Segment {
    fn is_empty(&self) -> bool {
        match self {
            &Segment::Fixed(ref fixed) if fixed.len() == 0 => true,
            _ => false
        }
    }
}

/// A route is an absolute URL pattern with a leading slash, and segments separated by slashes.
///
/// A segment that begins with a colon declares a variable, for example "/:user_id".
pub struct Route {
    segments: Vec<Segment>,
    callback: Callback
}

/// Returns a vector of segments from the given string.
fn get_segments(from: &str) -> Result<Vec<Segment>, &str> {
    if from.len() == 0 {
        return Err("route must not be empty");
    }
    if &from[0..1] != "/" {
        return Err("route must begin with a slash");
    }

    let stripped = &from[1..];
    Ok(stripped.split('/').map(|segment| if segment.len() > 0 && segment.as_bytes()[0] == b':' {
            Segment::Variable(segment[1..].to_string())
        } else {
            Segment::Fixed(segment.to_string())
        }
    ).collect::<Vec<Segment>>())
}

impl Route {
    fn new(from: &str, callback: Callback) -> Result<Route, &str> {
        Ok(Route {
            segments: try!(get_segments(from)),
            callback: callback
        })
    }
}

use std::fmt::{self, Debug, Formatter};

impl Debug for Route {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self.segments)
    }
}

/// Router structure
pub struct Router<T> {
    inner: RouterAny,
    _marker: PhantomData<T>
}

impl<T: Default + Any + Send> Router<T> {
    /// wraps the value returned by Default::default into a box
    fn create() -> Box<Any + Send> {
        Box::new(T::default())
    }

    /// Creates a new router.
    pub fn new() -> Router<T> {
        Router {
            inner: RouterAny::new::<T>(),
            _marker: PhantomData
        }
    }

    pub fn add_middleware(&mut self, middleware: TypedMiddleware<T>) {
        self.inner.middleware.push(Box::new(move |any, req| {
            if let Some(app) = any.downcast_mut::<T>() {
                middleware(app, req);
            }
        }))
    }

    /// Registers a callback for the given path for GET requests.
    #[inline]
    pub fn get(&mut self, path: &str, callback: TypedCallback<T>) {
        self.insert(Get, path, callback)
    }

    /// Registers a callback for the given path for POST requests.
    #[inline]
    pub fn post(&mut self, path: &str, callback: TypedCallback<T>) {
        self.insert(Post, path, callback)
    }

    /// Registers a callback for the given path for PUT requests.
    #[inline]
    pub fn put(&mut self, path: &str, callback: TypedCallback<T>) {
        self.insert(Put, path, callback)
    }

    /// Registers a callback for the given path for DELETE requests.
    #[inline]
    pub fn delete(&mut self, path: &str, callback: TypedCallback<T>) {
        self.insert(Delete, path, callback)
    }

    /// Registers a callback for the given path for HEAD requests.
    #[inline]
    pub fn head(&mut self, path: &str, callback: TypedCallback<T>) {
        self.insert(Head, path, callback)
    }

    /// Registers a static callback for the given path for GET requests.
    #[inline]
    pub fn get_static(&mut self, path: &str, callback: Static) {
        self.insert_static(Get, path, callback)
    }

    /// Inserts the given callback for the given method and given route.
    #[inline]
    pub fn insert(&mut self, method: Method, path: &str, callback: TypedCallback<T>) {
        self.insert_callback(method, path, Callback::Instance(Box::new(move |any, req, res| {
            if let Some(app) = any.downcast_mut::<T>() {
                callback(app, req, res);
            }
        })))
    }

    /// Registers a static callback for the given path for GET requests.
    #[inline]
    pub fn insert_static(&mut self, method: Method, path: &str, callback: Static) {
        self.insert_callback(method, path, Callback::Static(callback))
    }

    /// Inserts the given callback for the given method and given route.
    fn insert_callback(&mut self, method: Method, path: &str, callback: Callback) {
        let route = Route::new(path, callback).unwrap();
        info!("registered callback for {} (parsed as {:?})", path, route);

        self.inner.routes.entry(method).or_insert(Vec::new()).push(route)
    }
}

pub fn get_inner<T>(router: Router<T>) -> RouterAny {
    router.inner
}

/// Signature for a callback method
pub enum Callback {
    Instance(Box<Fn(&mut Any, &Request, Response)>),
    Static(Static)
}

unsafe impl Sync for Callback {}

pub type Middleware = Box<Fn(&mut Any, &mut Request)>;

/// Router structure
pub struct RouterAny {
    init: fn() -> Box<Any + Send>,
    prefix: Vec<Segment>,
    middleware: Vec<Middleware>,
    routes: HashMap<Method, Vec<Route>>
}

unsafe impl Sync for RouterAny {}

impl RouterAny {
    pub fn new<T: Default + Any + Send>() -> RouterAny {
        RouterAny {
            init: Router::<T>::create,
            prefix: Vec::new(),
            middleware: Vec::new(),
            routes: HashMap::new()
        }
    }

    /// Finds the first route (if any) that matches the given path, and returns the associated callback.
    pub fn find_callback(&self, req: &mut Request) -> Option<&Callback> {
        if self.match_prefix(req.path()) {
            debug!("{} {:?} matches prefix {:?}", req.method(), req.path(), self.prefix);
        } else {
            debug!("{} {:?} does not match prefix {:?}, skipping", req.method(), req.path(), self.prefix);
            return None;
        }

        if let Some(routes) = self.routes.get(req.method()) {
            let mut params = BTreeMap::new();
            let prefix_len = self.prefix.len();

            'top: for ref route in routes {
                let mut it_route = route.segments.iter();
                for actual in &req.path()[prefix_len..] {
                    match it_route.next() {
                        Some(&Segment::Fixed(ref fixed)) if fixed != actual => continue 'top,
                        Some(&Segment::Variable(ref name)) => {
                            params.insert(name.to_owned(), actual.to_string());
                        },
                        _ => ()
                    }
                }

                if it_route.next().is_none() {
                    request::set_params(req, params);
                    return Some(&route.callback);
                }

                params.clear();
            }

            warn!("no route matching method {} path {:?}", req.method(), req.path());
        } else {
            warn!("no routes registered for method {}", req.method());
        }

        None
    }

    /// Returns `true` if the given path matches this router's prefix.
    fn match_prefix(&self, path: &[String]) -> bool {
        if path.len() >= self.prefix.len() {
            // path is longer than prefix
            self.prefix.iter().zip(path.iter()).all(|(segment, component)| 
                match segment {
                    &Segment::Fixed(ref value) if value == component => true,
                    _ => false
                })
        } else {
            false
        }
    }

    pub fn new_instance(&self) -> Box<Any + Send> {
        (self.init)()
    }

    pub fn run_middleware(&self, app: &mut Any, req: &mut Request) {
        for middleware in &self.middleware {
            middleware(app, req);
        }
    }

    pub fn set_prefix(&mut self, prefix: &str) {
        let segments = get_segments(prefix).unwrap();
        if !(segments.len() == 1 && segments[0].is_empty()) {
            self.prefix = segments;
        }
    }
}
