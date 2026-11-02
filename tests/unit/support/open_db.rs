use rusqlite::Connection;

pub fn open_in_memory_for_test() -> Connection {
    let mut conn = Connection::open_in_memory().expect("open in-memory db");
    super::set_pragmas(&conn).expect("set pragmas");
    super::migrations::apply_migrations(&mut conn).expect("apply migrations");
    conn
}

pub fn item_status_for_test(conn: &Connection, uuid: &str) -> String {
    conn.query_row("SELECT status FROM items WHERE uuid = ?1", [uuid], |r| {
        r.get(0)
    })
    .expect("item exists")
}
