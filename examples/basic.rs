extern crate env_logger;
#[macro_use]
extern crate log;
extern crate edge;

use edge::{Edge, Cookie, Request, Response, Status};
use edge::header::AccessControlAllowOrigin;
use std::sync::atomic::{AtomicUsize, Ordering};

use std::collections::BTreeMap;
use edge::value;

struct MyApp {
    tmpl_path: String,
    counter: AtomicUsize
}

impl MyApp {

    fn new() -> MyApp {
        MyApp {
            tmpl_path: "toto".to_owned(),
            counter: AtomicUsize::new(0)
        }
    }

    fn home(&self, _req: &mut Request, mut res: Response) {
        let cnt = self.counter.fetch_add(1, Ordering::SeqCst);

        println!("in home, count = {}, path = {}", cnt, self.tmpl_path);

        res.content_type("text/html; charset=UTF-8").header(AccessControlAllowOrigin::Any);
        res.send("<html><head><title>home</title></head><body><h1>Hello, world!</h1></body></html>")
    }

    fn hello(&self,  req: &mut Request, mut res: Response) {
        let first_name = req.param("first_name").unwrap_or("John");
        let last_name = req.param("last_name").unwrap_or("Doe");

        let mut data = BTreeMap::new();
        data.insert("first_name", value::to_value(first_name));
        data.insert("last_name", value::to_value(last_name));

        res.content_type("text/plain; charset=UTF-8");
        res.render("views/hello.hbs", data)
    }

    fn settings(&self, req: &mut Request, mut res: Response) {
        let mut cookies = req.cookies();
        println!("name cookie: {}", cookies.find(|cookie| cookie.name == "name")
            .map_or("nope", |cookie| &cookie.value));

        //res.render(self.tmpl_path + "/sample.tpl", data)

        res.content_type("text/html; charset=UTF-8");
        res.send("<html><head><title>Settings</title></head><body><h1>Settings</h1></body></html>")
    }

    fn login(&self, req: &mut Request, mut res: Response) {
        let form = req.form().unwrap();
        match form.iter().find(|pair| pair.0 == "username").map(|pair| &pair.1) {
            None => (),
            Some(ref username) => {
                let mut cookie = Cookie::new("name".to_owned(), username.to_string());
                cookie.domain = Some("localhost".to_string());
                cookie.httponly = true;
                res.cookie(cookie);
            }
        }

        res.end(Status::NoContent)
    }

    fn files(&self, req: &mut Request, res: Response) {
        let path = req.path()[1..].join("/");
        res.send_file("web/".to_string() + &path)
    }

    fn redirect(&self, _req: &mut Request, res: Response) {
        use std::thread;
        use std::time::Duration;

        thread::spawn(|| {
            println!("waiting 3 seconds");
            thread::sleep(Duration::from_secs(3));
            res.redirect("http://google.com", None)
        });
    }

    fn streaming(&self, _req: &mut Request, res: Response) {
        use std::thread;
        use std::time::Duration;

        thread::spawn(move || {
            let mut res = res.stream();
            res.append("toto".as_bytes());
            thread::sleep(Duration::from_secs(1));

            res.append("tata".as_bytes());
            thread::sleep(Duration::from_secs(1));

            res.append("titi".as_bytes());
        });
    }

}

fn main() {
    env_logger::init().unwrap();

    let app = MyApp::new();
    let mut cter = Edge::new("0.0.0.0:3000", app);
    cter.get("/", MyApp::home);
    cter.get("/hello/:first_name/:last_name", MyApp::hello);
    cter.get("/settings", MyApp::settings);
    cter.get("/static", MyApp::files);

    cter.get("/redirect", MyApp::redirect);
    cter.get("/streaming", MyApp::streaming);

    cter.post("/login", MyApp::login);
    cter.start().unwrap();
}
