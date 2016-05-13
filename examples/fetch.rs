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

            let client = Client::new();
            client.request(&url);

            println!("url = {}", url);
            let mut res = res.stream();
            res.append(b"toto");
            thread::sleep(Duration::from_secs(1));
            res.append(b"tata");
            thread::sleep(Duration::from_secs(1));
            res.append(b"titi");
        });
    }

}

fn main() {
    let mut cter = Edge::new("0.0.0.0:3000", Fetch);
    cter.get("/", Fetch::home);
    cter.get("/fetch", Fetch::fetch);
    cter.start().unwrap();
}
