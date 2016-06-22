//! Defines functionality for a minimalistic synchronous client.

use hyper::{Client as HttpClient, Decoder, Encoder, Next};
use hyper::client::{Handler, Request as ClientRequest, Response as ClientResponse};
use hyper::net::HttpStream;

use std::thread::{self, Thread};

use buffer::Buffer;

pub struct Client {
    result: RequestResult
}

struct RequestResult {
    body: Option<Vec<u8>>,
    response: Option<ClientResponse>
}

impl RequestResult {
    fn new() -> RequestResult {
        RequestResult {
            body: None,
            response: None
        }
    }
}

impl Client {
    pub fn new() -> Client {
        Client {
            result: RequestResult::new()
        }
    }

    pub fn request(&mut self, url: &str) -> Vec<u8> {
        let client = HttpClient::new().unwrap();
        let _ = client.request(url.parse().unwrap(), ClientHandler::new(&mut self.result));

        // wait for request to complete
        thread::park();

        // close client and returns request body
        client.close();

        if let Some(buffer) = self.result.body.take() {
            buffer
        } else {
            Vec::new()
        }
    }

    pub fn status(&self) -> ::hyper::status::StatusCode {
        *self.result.response.as_ref().unwrap().status()
    }
}

struct ClientHandler {
    thread: Thread,
    buffer: Buffer,
    result: *mut RequestResult
}

unsafe impl Send for ClientHandler {}

impl ClientHandler {
    fn new(result: &mut RequestResult) -> ClientHandler {
        ClientHandler {
            thread: thread::current(),
            buffer: Buffer::new(),
            result: result as *mut RequestResult
        }
    }
}

impl Drop for ClientHandler {
    fn drop(&mut self) {
        unsafe { (*self.result).body = Some(self.buffer.take()); }

        // unlocks waiting thread
        self.thread.unpark();
    }
}

impl Handler<HttpStream> for ClientHandler {

    fn on_request(&mut self, _req: &mut ClientRequest) -> Next {
        Next::read()
    }

    fn on_request_writable(&mut self, _encoder: &mut Encoder<HttpStream>) -> Next {
        Next::read()
    }

    fn on_response(&mut self, res: ClientResponse) -> Next {
        use hyper::header::ContentLength;
        if let Some(&ContentLength(len)) = res.headers().get::<ContentLength>() {
            self.buffer.set_capacity(len as usize);
        }
        unsafe { (*self.result).response = Some(res); }

        Next::read()
    }

    fn on_response_readable(&mut self, decoder: &mut Decoder<HttpStream>) -> Next {
        if let Ok(keep_reading) = self.buffer.read_from(decoder) {
            if keep_reading {
                return Next::read();
            }
        }

        Next::end()
    }

    fn on_error(&mut self, err: ::hyper::Error) -> Next {
        println!("ERROR: {}", err);
        Next::remove()
    }

}
