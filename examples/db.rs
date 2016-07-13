extern crate env_logger;

#[macro_use]
extern crate log;

#[macro_use]
extern crate edge;
extern crate rusqlite;

use edge::{json, Edge, Router, Request, Response, Result, Status};
use edge::json::value::ToJson;

use rusqlite::Connection;
use rusqlite::Error;

use std::collections::BTreeMap;
use std::path::Path;
use std::{fs, result};

struct User {
    id: i32,
    name: String
}

#[derive(Default)]
struct Db;
impl Db {

    fn home(&mut self, req: &Request, _res: &mut Response) -> Result {
        let mut user_id = req.param("user_id").unwrap().to_string();
        user_id.trim();

        if user_id.len() == 0 {
            user_id = "1".to_string();
        }

        let connection = try!(Connection::open("db/demo.db").map_err(|e| (Status::InternalServerError, e.to_string())));
        let user = try!(connection.query_row("SELECT * FROM users WHERE user_id = ?", &[&user_id], |row|
            User {
                id: row.get(0),
                name: row.get(1)
            }
        ).map_err(|e| (Status::InternalServerError, match e {
            Error::QueryReturnedNoRows => format!("no user known with id {}", user_id),
            _ => format!("error when requesting user: {}", e)
        })));

        let mut data = BTreeMap::new();
        data.insert("id", json::to_value(&user.id));
        data.insert("name", json::to_value(&user.name));
        ok!("db", data.to_json())
    }

}

fn check_db() -> result::Result<(), Error> {
    let db = Path::new("db");
    if !db.exists() {
        fs::create_dir(db).unwrap();
    }

    let connection = try!(Connection::open("db/demo.db"));
    try!(connection.execute_batch("CREATE TABLE IF NOT EXISTS users(user_id INTEGER PRIMARY KEY, name TEXT);"));
    let num_users: i32 = try!(connection.query_row("SELECT COUNT(*) FROM users", &[], |row| row.get(0)));
    if num_users == 0 {
        assert!(try!(connection.execute("INSERT INTO USERS VALUES(1, 'John Doe')", &[])) == 1);
    }

    Ok(())
}

fn main() {
    env_logger::init().unwrap();

    check_db().unwrap();

    let mut edge = Edge::new("0.0.0.0:3000");
    let mut router = Router::new();
    router.get("/:user_id", Db::home);
    edge.mount("/", router);
    edge.register_template("db");
    edge.start().unwrap();
}
