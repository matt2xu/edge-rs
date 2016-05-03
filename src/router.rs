
use hyper::Method;
use std::collections::{BTreeMap, HashMap};

use request::Request;
use Response;

/// Signature for a callback method
pub type Callback<T> = fn(&T, &mut Request, Response);

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
    routes: HashMap<Method, Vec<(Route, Callback<T>)>>
}

impl<T> Router<T> {
    pub fn new() -> Router<T> {
        Router {
            routes: HashMap::new()
        }
    }

    /// Finds the first route (if any) that matches the given path, and returns the associated callback.
    pub fn find_callback(&self, req: &mut Request) -> Option<Callback<T>> {
        println!("path: {:?}", req.path());
        if let Some(routes) = self.routes.get(req.method()) {
            let mut params = BTreeMap::new();

            'top: for &(ref route, ref callback) in routes.iter() {
                println!("route: {:?}", route);
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
                    req.set_params(params);
                    return Some(*callback);
                }

                params.clear();
            }

            println!("no route matching method {} path {:?}", req.method(), req.path());
        } else {
            println!("no routes registered for method {}", req.method());
        }

        None
    }

    pub fn insert(&mut self, method: Method, route: Route, callback: Callback<T>) {
        println!("register callback for route: {:?}", route);
        self.routes.entry(method).or_insert(Vec::new()).push((route, callback));
    }
}

/// Creates a Route from a &str.
impl<'a> Into<Route> for &'a str {
    fn into(self) -> Route {
        if self.len() == 0 {
            panic!("route must not be empty");
        }
        if &self[0..1] != "/" {
            panic!("route must begin with a slash");
        }

        let stripped = &self[1..];
        let route = Route {
            segments:
                stripped.split('/').map(|segment| if segment.len() > 0 && &segment[0..1] == ":" {
                        Segment::Variable(segment[1..].to_owned())
                    } else {
                        Segment::Fixed(segment.to_owned())
                    }
                ).collect::<Vec<Segment>>()
        };
        println!("into from {} to {:?}", self, route);
        route
    }
}
