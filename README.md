# edge

A Web framework for Rust.

[Documentation](http://matt2xu.github.io/edge-rs)

## Overview

Edge is a Web framework that is simple to use, with the most common things
you need out of the box, and flexible, supporting both synchronous and asynchronous
request handling styles; see the documentation for examples.

The crate exports the things that you often need from dependencies, such as headers (from `hyper`),
cookies (from `cookie`) and JSON serialization (from `serde_json`).

Please note that this is an early version, and the API is likely to evolve.

## Use Edge

In your Cargo.toml, add:

```toml
[dependencies.edge]
git = "https://github.com/matt2xu/edge-rs.git"
tag = "v0.2.0"
```

## Examples

You can see examples on the [documentation home page](http://matt2xu.github.io/edge-rs), as well as in the [examples](https://github.com/matt2xu/edge-rs/tree/v0.2.0/examples) folder in the source code.

## License

MIT
