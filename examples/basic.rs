extern crate edge;

use edge::{Container, Cookie, Request, Response};
use std::io::Result;
use std::sync::Mutex;

struct MyApp {
    tmpl_path: String,
    counter: Mutex<u32>
}

impl MyApp {

    fn new() -> MyApp {
        MyApp {
            tmpl_path: "toto".to_owned(),
            counter: Mutex::new(0)
        }
    }

    fn home(&self, req: &Request, mut res: Response) -> Result<()> {
        let mut cookies = req.cookies();
        println!("cookies: {}", cookies.find(|cookie| cookie.name == "name")
            .map_or("no name", |cookie| &cookie.value));

        let cnt = {
            let mut counter = self.counter.lock().unwrap();
            *counter += 1;
            *counter
        };
        
        res.cookie("name", "John", Some(|cookie: &mut Cookie| {
            cookie.domain = Some("localhost".to_string());
            cookie.httponly = true;
        }));

        println!("in home, count = {}, path = {}", cnt, self.tmpl_path);
        //res.render(self.tmpl_path + "/sample.tpl", data)
        //res.send("toto")

        // set everything manually because we're streaming
        res.set_len(80);
        res.set_type("text/html".to_owned());
        res.stream(|writer| writer.write("<html><head><title>home</title></head><body><h1>Hello, world!</h1></body></html>".as_bytes()))
    }

}

fn main() {
    let app = MyApp::new();
    let mut cter = Container::new(app);
    cter.get("/", MyApp::home);
    cter.start("0.0.0.0:3000").unwrap();
}
