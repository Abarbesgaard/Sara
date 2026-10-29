//! Vector storage/latency bench for the sara vector-index spike.
//!
//! Compares, at N = 1k / 10k / 100k random unit vectors of sara's dim (256):
//!   json    vectors as JSON text, parsed every query (what sara does today)
//!   blob    little-endian f32 BLOB, decoded every query
//!   cached  vectors parsed once and kept in memory (long-lived MCP server)
//!   vec0    sqlite-vec virtual table, KNN in SQL (`MATCH ? AND k = 10`)
//!
//! Run: cargo run --release [-- 1000 10000 100000]
use rusqlite::{Connection, params};
use std::time::Instant;

const DIM: usize = 256;
const K: usize = 10;
const QUERIES: usize = 30;

struct Rng(u64);
impl Rng {
    fn next_f32(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    }
    fn unit(&mut self) -> Vec<f32> {
        let mut v: Vec<f32> = (0..DIM).map(|_| self.next_f32()).collect();
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter_mut().for_each(|x| *x /= n);
        v
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn top_k(scored: impl Iterator<Item = (i64, f32)>) -> Vec<i64> {
    let mut v: Vec<(i64, f32)> = scored.collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1));
    v.truncate(K);
    v.into_iter().map(|(i, _)| i).collect()
}

fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn unblob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn p50_ms(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(f64::total_cmp);
    xs[xs.len() / 2]
}

fn file_mb(conn: &Connection, table: &str) -> f64 {
    let bytes: i64 = conn
        .query_row(
            "select coalesce(sum(pgsize),0) from dbstat where name like ?1",
            [format!("{table}%")],
            |r| r.get(0),
        )
        .unwrap_or(0);
    bytes as f64 / 1e6
}

#[cfg(feature = "vec")]
fn register_vec() {
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
}

fn bench(n: usize) {
    let dir = std::env::temp_dir().join(format!("vector-bench-{n}.db"));
    let _ = std::fs::remove_file(&dir);
    let conn = Connection::open(&dir).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE ej (id INTEGER PRIMARY KEY, vector_json TEXT NOT NULL);
         CREATE TABLE eb (id INTEGER PRIMARY KEY, v BLOB NOT NULL);",
    )
    .unwrap();
    #[cfg(feature = "vec")]
    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE ev USING vec0(embedding float[{DIM}] distance_metric=cosine);"
    ))
    .unwrap();

    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let vectors: Vec<Vec<f32>> = (0..n).map(|_| rng.unit()).collect();
    let queries: Vec<Vec<f32>> = (0..QUERIES).map(|_| rng.unit()).collect();

    let insert = |sql: &str, f: &dyn Fn(&[f32]) -> rusqlite::types::Value| -> f64 {
        let t = Instant::now();
        let tx = conn.unchecked_transaction().unwrap();
        {
            let mut st = tx.prepare(sql).unwrap();
            for (i, v) in vectors.iter().enumerate() {
                st.execute(params![i as i64 + 1, f(v)]).unwrap();
            }
        }
        tx.commit().unwrap();
        t.elapsed().as_secs_f64() * 1000.0
    };
    let ins_json = insert("INSERT INTO ej VALUES (?1, ?2)", &|v| {
        serde_json::to_string(v).unwrap().into()
    });
    let ins_blob = insert("INSERT INTO eb VALUES (?1, ?2)", &|v| blob(v).into());
    #[cfg(feature = "vec")]
    let ins_vec = insert("INSERT INTO ev(rowid, embedding) VALUES (?1, ?2)", &|v| {
        blob(v).into()
    });

    let exact: Vec<Vec<i64>> = queries
        .iter()
        .map(|q| {
            top_k(
                vectors
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i as i64 + 1, dot(q, v))),
            )
        })
        .collect();

    let time = |f: &mut dyn FnMut(&[f32]) -> Vec<i64>| -> (f64, f64) {
        let mut ms = vec![];
        let mut agree = 0usize;
        for (q, ex) in queries.iter().zip(&exact) {
            let t = Instant::now();
            let got = f(q);
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            agree += got.iter().filter(|i| ex.contains(i)).count();
        }
        (p50_ms(ms), agree as f64 / (QUERIES * K) as f64)
    };

    let json = time(&mut |q| {
        let mut st = conn
            .prepare_cached("SELECT id, vector_json FROM ej")
            .unwrap();
        let rows = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .unwrap();
        top_k(rows.map(|r| {
            let (i, s) = r.unwrap();
            let v: Vec<f32> = serde_json::from_str(&s).unwrap();
            (i, dot(q, &v))
        }))
    });
    let blobq = time(&mut |q| {
        let mut st = conn.prepare_cached("SELECT id, v FROM eb").unwrap();
        let rows = st
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))
            .unwrap();
        top_k(rows.map(|r| {
            let (i, b) = r.unwrap();
            (i, dot(q, &unblob(&b)))
        }))
    });
    let cached = time(&mut |q| {
        top_k(
            vectors
                .iter()
                .enumerate()
                .map(|(i, v)| (i as i64 + 1, dot(q, v))),
        )
    });
    #[cfg(feature = "vec")]
    let vec0 = time(&mut |q| {
        let mut st = conn
            .prepare_cached("SELECT rowid FROM ev WHERE embedding MATCH ?1 AND k = ?2")
            .unwrap();
        st.query_map(params![blob(q), K as i64], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    });

    let row = |name: &str, ins: f64, (p50, agree): (f64, f64), mb: f64| {
        println!("| {n} | {name} | {mb:.1} | {ins:.0} | {p50:.2} | {agree:.2} |");
    };
    row("json (today)", ins_json, json, file_mb(&conn, "ej"));
    row("f32 blob", ins_blob, blobq, file_mb(&conn, "eb"));
    row("in-memory cache", 0.0, cached, 0.0);
    #[cfg(feature = "vec")]
    row("sqlite-vec vec0", ins_vec, vec0, file_mb(&conn, "ev"));
    drop(conn);
    let _ = std::fs::remove_file(&dir);
}

fn main() {
    #[cfg(feature = "vec")]
    register_vec();
    let sizes: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let sizes = if sizes.is_empty() {
        vec![1_000, 10_000, 100_000]
    } else {
        sizes
    };
    println!("| N | storage | MB on disk | insert ms | query p50 ms | top-10 agreement |");
    println!("|---:|---|---:|---:|---:|---:|");
    for n in sizes {
        bench(n);
    }
}
