use std::cell::RefCell;
use std::sync::{Arc, RwLock};

struct Resp {
    body: Vec<u8>,
    locked: RwLock<Vec<u8>>
}

impl Resp {
    fn new() -> Resp {
        Resp {
            body: Vec::new(),
            locked: RwLock::new(Vec::new())
        }
    }

    fn send(&mut self, content: &[u8]) {
        self.body.extend_from_slice(content);
    }

    fn send_lock(&self, content: &[u8]) {
        self.locked.write().unwrap().extend_from_slice(content);
    }
}

struct Response {
    inner: Arc<Resp>
}

impl Response {
    pub fn new() -> Response {
        Response {
            inner: Arc::new(Resp::new())
        }
    }

    fn send(&mut self, content: &[u8]) {
        if let Some(resp) = Arc::get_mut(&mut self.inner) {
            // same thread, can borrow mutably
            resp.send(content);
            return;
        }

        // cloned
        self.inner.send_lock(content);
    }
}

impl Clone for Response {
    fn clone(&self) -> Response {
        Response {
            inner: self.inner.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Resp, Response};
    use std::thread;
    use std::time::Duration;
    use std::sync::Arc;

    fn callback(res: &mut Response) {
        res.send(b"tata");
    }

    fn callback_async(res: &mut Response) {
        let mut res = res.clone();
        thread::spawn(move || {
            println!("waiting for 1 second");
            thread::sleep(Duration::from_secs(1));
            println!("sending toto");
            res.send(b"toto");

            // normally invoked indirectly when res says ctrl.ready()
            response_writable(&res.inner);
        });
        println!("after thread spawn");
    }

    #[test]
    fn test() {
        let mut res = Response::new();
        callback(&mut res);
        assert!(Arc::get_mut(&mut res.inner).is_some());

        //if let Some(resp) = Arc::get_mut(&mut res.inner) {
            println!("content: {:?}", res.inner.body);
        //}
    }

    fn response_writable(resp: &Resp) {
        println!("content: {:?}", *resp.locked.read().unwrap());
    }

    #[test]
    fn test_async() {
        let mut res = Response::new();
        callback_async(&mut res);
        assert!(Arc::get_mut(&mut res.inner).is_none());

        println!("waiting for 3 seconds");
        thread::sleep(Duration::from_secs(3));
    }
}
