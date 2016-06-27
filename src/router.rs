
use hyper::Method;
use std::collections::{BTreeMap, HashMap};

use request;
use request::Request;
use response::Response;

use url::Url;

pub type Instance<T> = fn(&mut T, &Request, Response);
pub type Static = fn(&Request, Response);

/// Signature for a callback method
pub enum Callback<T> {
    Instance(Instance<T>),
    Static(Static)
}

/// A segment is either a fixed string, or a variable with a name
#[derive(Debug, Clone)]
enum Segment {
    Fixed(String),
    Variable(String)
}

/// A route is an absolute URL pattern with a leading slash, and segments separated by slashes.
///
/// A segment that begins with a colon declares a variable, for example "/:user_id".
#[derive(Debug)]
pub struct Route {
    segments: Vec<Segment>
}

/// Router structure
pub struct Router<T> {
    pub base_url: Url,
    routes: HashMap<Method, Vec<(Route, Callback<T>)>>
}

impl<T> Router<T> {
    pub fn new(addr: &str) -> Router<T> {
        Router {
            base_url: Url::parse(&("http://".to_string() + addr)).unwrap(),
            routes: HashMap::new()
        }
    }

    /// Finds the first route (if any) that matches the given path, and returns the associated callback.
    pub fn find_callback(&self, req: &mut Request) -> Option<&Callback<T>> {
        info!("path: {:?}", req.path());
        if let Some(routes) = self.routes.get(req.method()) {
            let mut params = BTreeMap::new();

            'top: for &(ref route, ref callback) in routes.iter() {
                let mut it_route = route.segments.iter();
                for actual in req.path() {
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
                    return Some(callback);
                }

                params.clear();
            }

            warn!("no route matching method {} path {:?}", req.method(), req.path());
        } else {
            warn!("no routes registered for method {}", req.method());
        }

        None
    }

    pub fn insert(&mut self, method: Method, path: &str, callback: Callback<T>) {
        let route = path.parse().unwrap();
        info!("registered callback for {} (parsed as {:?})", path, route);
        self.routes.entry(method).or_insert(Vec::new()).push((route, callback));
    }
}

impl<T> From<Instance<T>> for Callback<T> {
    fn from(function: Instance<T>) -> Callback<T> {
        Callback::Instance(function)
    }
}

impl<T> From<Static> for Callback<T> {
    fn from(function: Static) -> Callback<T> {
        Callback::Static(function)
    }
}

/// Creates a Route from a &str.
impl ::std::str::FromStr for Route {
    type Err = &'static str;
    fn from_str(from: &str) -> Result<Self, Self::Err> {
        if from.len() == 0 {
            return Err("route must not be empty");
        }
        if &from[0..1] != "/" {
            return Err("route must begin with a slash");
        }

        let stripped = &from[1..];
        Ok(Route {
            segments:
                stripped.split('/').map(|segment| if segment.len() > 0 && &segment[0..1] == ":" {
                        Segment::Variable(segment[1..].to_owned())
                    } else {
                        Segment::Fixed(segment.to_owned())
                    }
                ).collect::<Vec<Segment>>()
        })
    }
}
