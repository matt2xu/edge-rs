extern crate env_logger;
#[macro_use]
extern crate log;
extern crate edge;

use edge::{Edge, Request, Response, Status, Client};
use edge::value;

use std::collections::BTreeMap;

struct Fetch;
impl Fetch {

    fn home(&self, _req: &mut Request, mut res: Response) {
        res.content_type("text/html");
        res.render("fetch", BTreeMap::<String, value::Value>::new())
    }

    fn fetch(&self, req: &mut Request, res: Response) {
        let url = req.query("url").unwrap_or("http://google.com").to_string();

        use std::thread;
        use std::time::Duration;

        thread::spawn(move || {
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
        });
    }

}

fn main() {
    env_logger::init().unwrap();

    let mut edge = Edge::new("0.0.0.0:3000", Fetch);
    edge.get("/", Fetch::home);
    edge.get("/fetch", Fetch::fetch);
    edge.register_template("fetch");
    edge.start().unwrap();
}
