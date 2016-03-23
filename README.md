# edge

A Web framework for Rust.

[Documentation](http://matt2xu.github.io/edge-rs)

## Overview

Edge is a Web framework that aims to be simple to use, with the most common things you need out of the box. There are no plugins, the framework is not modular, but it is simple to use and easy to contribute to.

The crate exports the things that you often need from dependencies, such as headers (from `hyper`), cookies (from `cookie`) and JSON serialization (from `serde_json`).

*Warning*: this is a very early version, and the API is not fully stable yet.

## Example

```rust
extern crate edge;

use edge::{Container, Request, Response, Status};
use edge::header::Server;
use std::io::Result;
use std::sync::Mutex;

struct MyApp {
    version: &'static str,
    counter: Mutex<u32>
}

impl MyApp {
    fn home(&self, _req: &mut Request, mut res: Response) -> Result<()> {
        let cnt = {
            let mut counter = self.counter.lock().unwrap();
            *counter += 1;
            *counter
        };

        res.status(Status::Ok).content_type("text/plain");
        res.header(Server(format!("Edge version {}", self.version)));
        res.send(format!("Hello, world! {} visits", cnt))
    }
}

fn main() {
    let app = MyApp { version: "0.1", counter: Mutex::new(0) };
    let mut cter = Container::new(app);
    cter.get("/", MyApp::home);
    cter.start("0.0.0.0:3000").unwrap();
}
```

## License

MIT
