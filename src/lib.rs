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

#[cfg(test)]
mod tests {

    use super::{App, Router};

    #[test]
    fn test_app() {
        #[derive(Default)]
        struct MyApp {
            counter: u32
        }

        impl MyApp {
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
