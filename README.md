# edge

A Web framework for Rust.

[Documentation](http://matt2xu.github.io/edge-rs)

## Overview

Edge is a Web framework that is simple to use, with the most common things
you need out of the box, and flexible, supporting both synchronous and asynchronous
request handling styles; see below for examples.

The crate exports the things that you often need from dependencies, such as headers (from `hyper`),
cookies (from `cookie`) and JSON serialization (from `serde_json`).

Please note that this is an early version, and the API is likely to evolve.

## Use Edge

In your Cargo.toml, add:

```toml
[dependencies.edge]
git = "https://github.com/matt2xu/edge-rs.git"
tag = "v0.1.0"
```

## Hello World

The most basic application: no state, a single page that prints Hello, world!

```rust
extern crate edge;

use edge::{Edge, Request, Response};

struct MyApp;
impl MyApp {
    fn hello(&self, _req: &mut Request, mut res: Response) {
        res.content_type("text/plain");
        res.send("Hello, world!")
    }
}

fn main() {
    let mut cter = Edge::new(MyApp);
    cter.get("/", MyApp::hello);
    cter.start("0.0.0.0:3000").unwrap();
}
```

## Asynchronous handling

Under the hood, Edge uses the asynchronous version of Hyper. This means that to get the maximum
performance, you should avoid waiting in a handler, so that other requests
can be served as soon as possible. In that example, the handler waits in a separate thread before sending
the response.

```rust
extern crate edge;

use edge::{Edge, Request, Response};
use std::thread;
use std::time::Duration;

struct MyApp;
impl MyApp {
    fn hello(&self, _req: &mut Request, mut res: Response) {
        thread::spawn(move || {
            println!("waiting 1 second");
            thread::sleep(Duration::from_secs(1));

            res.content_type("text/plain");
            res.send("Hello, world!")
        });

        // the handler returns immediately without waiting for the thread
    }
}

fn main() {
    let mut cter = Edge::new(MyApp);
    cter.get("/", MyApp::hello);
    cter.start("0.0.0.0:3000").unwrap();
}
```

## Templating

Here our application has a version, still a single handler except this time
it accepts any page name, and renders a Handlebars template.  We're also
setting a custom Server header.

```rust
extern crate edge;

use edge::{Edge, Request, Response, Status};
use edge::header::Server;
use std::collections::BTreeMap;

struct MyApp {
    version: &'static str
}

impl MyApp {
    fn page_handler(&self, req: &mut Request, mut res: Response) {
        let mut data = BTreeMap::new();
        data.insert("title", req.param("page").unwrap());
        data.insert("version", self.version);

        res.content_type("text/html").header(Server(format!("Edge version {}", self.version)));
        res.render("views/page.hbs", data)
    }
}

fn main() {
    let app = MyApp { version: "0.1" };
    let mut cter = Edge::new(app);
    cter.get("/:page", MyApp::page_handler);
    cter.start("0.0.0.0:3000").unwrap();
}
```

## Using a shared mutable counter

In this example, we use an atomic integer to track a counter. This shows a very basic
kind of shared state for a handler. In practice, it's best to avoid using blocking
mechanisms (locks, mutexes) in a handler directly. Prefer non-blocking calls,
like channels' try_recv, or move blocking code in a separate thread,
see the example for asynchronous handling above.

```rust
extern crate edge;

use edge::{Edge, Request, Response, Status};
use std::sync::atomic::{AtomicUsize, Ordering};

struct MyApp {
    counter: AtomicUsize
}

impl MyApp {
    fn home(&self, _req: &mut Request, mut res: Response) {
        let visits = self.counter.load(Ordering::Relaxed);
        self.counter.store(visits + 1, Ordering::Relaxed);

        res.status(Status::Ok).content_type("text/plain");
        res.send(format!("Hello, world! {} visits", visits))
    }
}

fn main() {
    let app = MyApp { counter: AtomicUsize::new(0) };
    let mut cter = Edge::new(app);
    cter.get("/", MyApp::home);
    cter.start("0.0.0.0:3000").unwrap();
}
```

## License

MIT
