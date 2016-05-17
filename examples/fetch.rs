extern crate edge;

use edge::{Edge, Request, Response, Client};

use std::collections::BTreeMap;
use edge::value;

struct Fetch;
impl Fetch {

    fn home(&self, _req: &mut Request, mut res: Response) {
        res.content_type("text/html");
        res.render("views/fetch.hbs", BTreeMap::<String, value::Value>::new())
    }

    fn fetch(&self, req: &mut Request, res: Response) {
        let url = req.query("url").unwrap_or("http://google.com").to_string();

        use std::thread;
        use std::time::Duration;

        thread::spawn(move || {
            thread::sleep(Duration::from_secs(1));

            let mut client = Client::new();
            let mut res = res.stream();
            println!("url = {}", url);
            client.request(&url, move |buffer| {
                println!("got {} bytes", buffer.len());
                res.append(buffer);
                thread::sleep(Duration::from_secs(1));
            });
        });
    }

}

fn main() {
    let mut cter = Edge::new("0.0.0.0:3000", Fetch);
    cter.get("/", Fetch::home);
    cter.get("/fetch", Fetch::fetch);
    cter.start().unwrap();
}
