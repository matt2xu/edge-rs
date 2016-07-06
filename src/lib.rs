use std::any::Any;
use std::boxed::Box;
use std::marker::PhantomData;

/// provides a type safe interface
/// guarantees T is same for all calls to register
pub struct Router<T> {
    inner: InnerRouter,
    _marker: PhantomData<T>
}

impl<T: Default + Any> Router<T> {
    /// wraps the value returned by Default::default into a box
    fn create() -> Box<Any> {
        Box::new(T::default())
    }

    pub fn new() -> Router<T> {
        Router {
            inner: InnerRouter::new::<T>(),
            _marker: PhantomData
        }
    }

    pub fn register(&mut self, callback: fn(&mut T)) {
        self.inner.routes.push(Box::new(move |any| {
            if let Some(app) = any.downcast_mut::<T>() {
                callback(app);
            }
        }));
    }
}


struct InnerRouter {
    init: fn() -> Box<Any>,
    routes: Vec<Box<Fn(&mut Any)>>
}

impl InnerRouter {
    fn new<T: Default + Any>() -> InnerRouter {
        InnerRouter {
            init: Router::<T>::create,
            routes: Vec::new()
        }
    }

    pub fn test(&self) {
        let mut app_box = (self.init)();
        for route in &self.routes {
            route(app_box.as_mut());
        }
    }
}

pub struct App {
    routers: Vec<InnerRouter>
}

impl App {
    pub fn new() -> App {
        App {
            routers: Vec::new()
        }
    }

    pub fn add_router<T>(&mut self, router: Router<T>) {
        self.routers.push(router.inner);
    }

    pub fn test_routers(&self) {
        for router in &self.routers {
            router.test();
        }
    }
}

pub struct Response {
    status: u32
}

impl Response {
    pub fn new() -> Response {
        Response {
            status: 500
        }
    }

    pub fn status(&mut self, status: u32) {
        println!("status was {}, set to {}", self.status, status);
        self.status = status;
    }
}

pub struct Stream;

impl Stream {
    pub fn write<I: AsRef<[u8]>>(&mut self, bytes: I) {
        println!("bytes: {:?}", bytes.as_ref());
    }
}

enum HandlerResult<T> {
    End(u32),
    Streaming(Box<Fn(&mut T, &mut Stream)>)
}

impl<T> HandlerResult<T> {
    pub fn streaming(closure: Box<Fn(&mut T, &mut Stream)>) -> HandlerResult<T> {
        HandlerResult::Streaming(closure)
    }
}

impl<T> From<Box<Fn(&mut T, &mut Stream)>> for HandlerResult<T> {
    fn from(closure: Box<Fn(&mut T, &mut Stream)>) -> HandlerResult<T> {
        HandlerResult::Streaming(closure)
    }
}

impl<T> From<u32> for HandlerResult<T> {
    fn from(status: u32) -> HandlerResult<T> {
        HandlerResult::End(status)
    }
}

#[cfg(test)]
mod tests {

    use super::{App, HandlerResult, Router, Response, Stream};

    use std::boxed::Box;

    #[test]
    fn test_app() {
        #[derive(Default)]
        struct MyApp {
            counter: u32
        }

        impl MyApp {
            fn new_handler(&mut self, res: &mut Response) -> Option<HandlerResult<Self>> {
                res.status(200);
                Some(HandlerResult::streaming(Box::new(|this, stream| {
                    println!("counter = {}", this.counter);
                    stream.write("48");
                })))
            }

            fn new_handler2(&mut self) -> Option<HandlerResult<Self>> {
                Some(200.into())
            }

            fn handler(&mut self) {
                println!("MyApp::handler");
                self.counter += 1;
                println!("MyApp::handler, counter = {}", self.counter);
            }

            fn handler2(&mut self) {
                println!("MyApp::handler2");
                self.counter += 1;
                println!("MyApp::handler2, counter = {}", self.counter);
            }
        }

        let mut app = App::new();

        let mut my_app = MyApp::default();
        let mut response = Response::new();
        let result = my_app.new_handler(&mut response);
        if let Some(res) = result {
            if let HandlerResult::Streaming(closure) = res.into() {
                let mut stream = Stream;
                closure(&mut my_app, &mut stream);
            }
        }

        let mut router = Router::new();
        router.register(MyApp::handler);
        router.register(MyApp::handler2);
        app.add_router(router);

        #[derive(Default)]
        struct MyApp2 {
            empty: String
        }

        impl MyApp2 {
            fn another(&mut self) {
                println!("MyApp2::another empty = {}", self.empty);
                self.empty.push_str("Hello, world!");
                println!("MyApp2::another empty = {}", self.empty);
            }
        }


        let mut router = Router::new();
        router.register(MyApp2::another);
        app.add_router(router);

        app.test_routers();
    }

}
