extern crate edge;

use edge::{Container, Cookie, Request, Response, Status};
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

    fn home(&self, _req: &mut Request, mut res: Response) -> Result<()> {
        let cnt = {
            let mut counter = self.counter.lock().unwrap();
            *counter += 1;
            *counter
        };

        println!("in home, count = {}, path = {}", cnt, self.tmpl_path);

        // set length manually because we're streaming
        res.set_status(Status::Ok);
        res.set_len(80);
        res.set_type("text/html");
        res.stream(|writer| writer.write("<html><head><title>home</title></head><body><h1>Hello, world!</h1></body></html>".as_bytes()))
    }

    fn hello(&self,  req: &mut Request, mut res: Response) -> Result<()> {
        let first_name = req.params().find(|&&(ref k, _)| k == "first_name").map_or("John", |pair| &pair.1);
        let last_name = req.params().find(|&&(ref k, _)| k == "last_name").map_or("Doe", |pair| &pair.1);
        res.set_type("text/plain");
        res.send(format!("hello {} {}!", first_name, last_name))
    }

    fn settings(&self, req: &mut Request, mut res: Response) -> Result<()> {
        let mut cookies = req.cookies();
        println!("name cookie: {}", cookies.find(|cookie| cookie.name == "name")
            .map_or("nope", |cookie| &cookie.value));

        //res.render(self.tmpl_path + "/sample.tpl", data)

        res.set_type("text/html");
        res.send("<html><head><title>Settings</title></head><body><h1>Settings</h1></body></html>")
    }

    fn login(&self, req: &mut Request, mut res: Response) -> Result<()> {
        let form = try!(req.form());
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

    fn files(&self, req: &mut Request, res: Response) -> Result<()> {
        let path = req.path()[1..].join("/");
        res.send_file("web/".to_string() + &path)
    }

}

fn main() {
    let app = MyApp::new();
    let mut cter = Container::new(app);
    cter.get("/", MyApp::home);
    cter.get("/hello/:first_name/:last_name", MyApp::hello);
    cter.get("/settings", MyApp::settings);
    cter.get("/static", MyApp::files);
    cter.post("/login", MyApp::login);
    cter.start("0.0.0.0:3000").unwrap();
}
