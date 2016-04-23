extern crate edge;

use edge::{Edge, Cookie, Request, Response, Status};
use edge::header::AccessControlAllowOrigin;
use std::io::Result;
use std::sync::Mutex;

use std::collections::BTreeMap;
use edge::value;

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

    fn home(&self, _req: &mut Request, mut res: Response) {
        let cnt = {
            let mut counter = self.counter.lock().unwrap();
            *counter += 1;
            *counter
        };

        println!("in home, count = {}, path = {}", cnt, self.tmpl_path);

        // set length manually because we're streaming
        res.status(Status::Ok).len(80).content_type("text/html").header(AccessControlAllowOrigin::Any);
        res.send("<html><head><title>home</title></head><body><h1>Hello, world!</h1></body></html>")
    }

    fn hello(&self,  req: &mut Request, mut res: Response) {
        let first_name = req.params().find(|&&(ref k, _)| k == "first_name").map_or("John", |pair| &pair.1);
        let last_name = req.params().find(|&&(ref k, _)| k == "last_name").map_or("Doe", |pair| &pair.1);

        let mut data = BTreeMap::new();
        data.insert("first_name", value::to_value(first_name));
        data.insert("last_name", value::to_value(last_name));

        res.content_type("text/plain");
        res.render("views/hello.hbs", data)
    }

    fn settings(&self, req: &mut Request, mut res: Response) {
        let mut cookies = req.cookies();
        println!("name cookie: {}", cookies.find(|cookie| cookie.name == "name")
            .map_or("nope", |cookie| &cookie.value));

        //res.render(self.tmpl_path + "/sample.tpl", data)

        res.content_type("text/html");
        res.send("<html><head><title>Settings</title></head><body><h1>Settings</h1></body></html>")
    }

    fn login(&self, req: &mut Request, mut res: Response) {
        let form = req.form().unwrap();
        match form.iter().find(|pair| pair.0 == "username").map(|pair| &pair.1) {
            None => (),
            Some(ref username) => {
                res.cookie("name", &username, Some(|cookie: &mut Cookie| {
                    cookie.domain = Some("localhost".to_string());
                    cookie.httponly = true;
                }));
            }
        }

        res.end(Status::NoContent)
    }

    fn files(&self, req: &mut Request, res: Response) {
        let path = req.path()[1..].join("/");
        res.send_file("web/".to_string() + &path)
    }

    fn redirect(&self, _req: &mut Request, res: Response) {
        res.redirect("http://google.com", None)
    }

}

fn main() {
    let app = MyApp::new();
    let mut cter = Edge::new(app);
    cter.get("/", MyApp::home);
    cter.get("/hello/:first_name/:last_name", MyApp::hello);
    cter.get("/settings", MyApp::settings);
    cter.get("/static", MyApp::files);

    cter.get("/redirect", MyApp::redirect);

    cter.post("/login", MyApp::login);
    cter.start("0.0.0.0:3000").unwrap();
}
