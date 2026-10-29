//! Smallest program that opens SQLite, with and without sqlite-vec linked in.
//! `cargo build --release --bin size_probe [--no-default-features]` and diff sizes.
fn main() {
    #[cfg(feature = "vec")]
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    }
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let v: String = conn
        .query_row("select sqlite_version()", [], |r| r.get(0))
        .unwrap();
    println!("{v}");
}
