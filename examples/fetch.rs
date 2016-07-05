extern crate env_logger;
#[macro_use]
extern crate log;
extern crate edge;

use edge::{Edge, Router, Request, Response, Status, Client};
use edge::value;

use std::collections::BTreeMap;
use std::thread;
use std::time::Duration;

#[derive(Default)]
struct Home;
impl Home {

    fn home(&mut self, _req: &Request, mut res: Response) {
        res.content_type("text/html");
        res.render("fetch", BTreeMap::<String, value::Value>::new())
    }

}

#[derive(Default)]
struct Fetch;
impl Fetch {

    fn home(&mut self, _req: &Request, res: Response) {
        res.redirect("/", None)
    }

    fn fetch(&mut self, req: &Request, res: Response) {
        let url = req.query("url").unwrap_or("http://google.com").to_string();

        thread::sleep(Duration::from_secs(1));

        let mut client = Client::new();
        let mut stream = res.stream();
        println!("url = {}", url);

        let buffer = client.request(&url);
        if client.status() == Status::Ok {
            println!("got {} bytes", buffer.len());
            stream.append(buffer);
        }

        thread::sleep(Duration::from_secs(1));

        let buffer = client.request(&url);
        if client.status() == Status::Ok {
            println!("got {} bytes", buffer.len());
            stream.append(buffer);
        }
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
