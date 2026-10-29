use std::path::{Path, PathBuf};
use std::process::Command;

use criterion::{Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

const SEED_TASKS: usize = 200;
const SEED_MEMORIES: usize = 100;

struct Sara {
    home: TempDir,
    project: PathBuf,
}

impl Sara {
    fn seeded() -> Self {
        let home = tempfile::tempdir().expect("tempdir");
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir project");
        let s = Sara { home, project };
        s.run(&["init", "--name", "Bench", "-y"]);
        for i in 0..SEED_TASKS {
            let desc = format!("Benchmark task {i} touching recall ranking and storage");
            s.run(&["add", "--yes", "-t", "bench", &desc]);
        }
        for i in 0..SEED_MEMORIES {
            let text = format!("Memory {i}: recall ranking keeps the best match per cluster");
            s.run(&["learn", "--tag", "bench", &text]);
        }
        s
    }

    fn cmd(&self) -> Command {
        let home: &Path = self.home.path();
        let mut c = Command::new(env!("CARGO_BIN_EXE_sara"));
        c.current_dir(&self.project)
            .env("HOME", home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("SARA_NO_TELEMETRY", "1")
            .env("NO_COLOR", "1");
        c
    }

    fn run(&self, args: &[&str]) {
        let out = self.cmd().args(args).output().expect("spawn sara");
        assert!(
            out.status.success(),
            "`sara {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn cli(c: &mut Criterion) {
    let sara = Sara::seeded();
    let mut group = c.benchmark_group("cli");
    group.sample_size(20);
    group.bench_function("list", |b| b.iter(|| sara.run(&["list"])));
    group.bench_function("recall_query", |b| {
        b.iter(|| sara.run(&["recall", "recall ranking"]))
    });
    group.bench_function("recall_tag", |b| {
        b.iter(|| sara.run(&["recall", "--tag", "bench"]))
    });
    group.finish();
}

criterion_group!(benches, cli);
criterion_main!(benches);
