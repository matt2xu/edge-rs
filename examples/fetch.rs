extern crate env_logger;
#[macro_use]
extern crate log;
#[macro_use]
extern crate edge;

use edge::{json, Edge, Router, Request, Response, Result, Status, stream, Client};
use edge::json::value::ToJson;

use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

#[derive(Default)]
struct Home;
impl Home {

    fn home(&mut self, _req: &Request, res: &mut Response) -> Result {
        res.content_type("text/html");
        ok!("fetch", BTreeMap::<String, json::Value>::new().to_json())
    }

}

#[derive(Default)]
struct Fetch;
impl Fetch {

    fn home(&mut self, _req: &Request, _res: &mut Response) -> Result {
        ok!(Status::Found, "/")
    }

    fn fetch(&mut self, req: &Request, _res: &mut Response) -> Result {
        let url = req.query("url").unwrap_or("http://google.com").to_string();
        stream(move |_app: &mut Self, writer| {
            thread::sleep(Duration::from_secs(1));

            let mut client = Client::new();
            println!("url = {}", url);

            let buffer = client.request(&url);
            if client.status() == Status::Ok {
                println!("got {} bytes", buffer.len());
                try!(writer.write(&buffer));
            }

            thread::sleep(Duration::from_secs(1));

            let buffer = client.request(&url);
            if client.status() == Status::Ok {
                println!("got {} bytes", buffer.len());
                try!(writer.write(&buffer));
            }

            Ok(())
        })
    }

}

fn main() {
    env_logger::init().unwrap();

    let mut edge = Edge::new("0.0.0.0:3000");

    let mut router = Router::new();
    router.get("/", Home::home);
    edge.mount("/", router);

    let mut router = Router::new();
    router.get("/", Fetch::home);
    router.get("/fetch", Fetch::fetch);
    edge.mount("/api/v1", router);

    edge.register_template("fetch");
    edge.start().unwrap();
}
