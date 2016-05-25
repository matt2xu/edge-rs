Version 0.2.0 (2016-05-25)
==========================

Features
--------

* now passing the hostname to the `Edge::new` constructor to avoid parsing the same base URL for each request. The base URL is now constructed once when edge is initialized.
* add a streamin API: an asynchronous handler can put a response in streaming mode, and append bytes as needed. Each call to `append` transfers bytes to the transport. Useful for on-the-fly construction of responses.
* added basic HTTP client
* added logging facility using [log](https://crates.io/crates/log) and [env_logger](https://crates.io/crates/env_logger)

Improvements
---------

* added more file extensions in Response::send_file
* made reading/writing more efficient: only requests reading/writing when the transport would need to block to read/write again
* now checking HTTP requests for proper values of Transfer-Encoding and Content-Length headers. This is a breaking change as the framework now ignores payload for methods GET/HEAD/DELETE/CONNECT, as this has no defined semantics. Please open an issue if you believe that edge should allow that.
* Request::query now takes a key and returns the associated value

Fixes
---------

  * now uses an atomic boolean in the response with atomic Compare-And-Swap operations to guarantee the absence of race conditions, which could cause a response to be indefinitely held back. This could happen in theory if a spawned thread would send the response at the *exact same time* as the framework would check to see whether that response has been sent. In the absence of atomic operations (or a lock), the framework would see that the response is not done yet and start waiting, while at the same time the response would see that it does not need to notify the transport since it has not been instructed to do so *yet* by the framework.

  This is solved by two concurrent CAS on the same atomic boolean, so that both threads have a coherent view.
  * fixed issue #5 by removing unsafe code that caused a use-after-free bug when a request was canceled. The issue was fixed by using Arc<UnsafeCell> instead of Box, so that the data is properly deallocated when the last owner is dropped.
  * removed Sync and Send trait requirement on the application structure (propagation of change in [hyper](http://hyper.rs))
  * fixed issue #4 removed unused dependency on `time` (contribution by @serprex)

Version 0.1.0 (2016-05-08)
==========================

Features
--------

* asynchronous I/O as implemented by the (now defunct) `mio` branch of [hyper](http://hyper.rs)
* support immediate/synchronous request handling as in version 0.0.1: a handler computes a response and sends it back to the client.
* also support deferred/asynchronous request handling: a handler can spawn a new thread and return immediately; the thread will do some work in the background before sending the response.

Fixes
---------

* fixed `Response::cookie` method, would not type check when used with `None`.
* fixed issue #1 by adding a `Request::param` in 75c5014
* fixed issue #2 by changing the return type of methods `path`, `query`, `fragment` in 28d3f8d

Version 0.0.1 (2016-03-23)
==========================

Features
--------

* synchronous request handling using [hyper](http://hyper.rs)
* routing
