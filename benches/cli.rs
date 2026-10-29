use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use tempfile::TempDir;

/// (tasks, memories) per seeded database. The large seed shows how commands
/// scale as a real user's memory grows; seeding it takes a couple of minutes.
const SEEDS: [(usize, usize); 2] = [(200, 100), (1000, 1000)];
const FILES: usize = 10;

struct Sara {
    home: TempDir,
    project: PathBuf,
}

impl Sara {
    fn seeded(tasks: usize, memories: usize) -> Self {
        let home = tempfile::tempdir().expect("tempdir");
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir project");
        let s = Sara { home, project };
        s.run(&["init", "--name", "Bench", "-y"]);
        for i in 0..tasks {
            let desc = format!("Benchmark task {i} touching recall ranking and storage");
            s.run(&["add", "--yes", "-t", "bench", &desc]);
        }
        for i in 0..memories {
            let file = format!("src/module_{}.rs", i % FILES);
            let text = format!("Memory {i}: recall ranking keeps the best match per cluster");
            s.run(&["learn", "--force", "--tag", "bench", "--file", &file, &text]);
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

    /// Times only `args`, then undoes it untimed so writes don't grow the seed.
    fn timed_then_undo(&self, iters: u64, args: &[&str]) -> Duration {
        let mut total = Duration::ZERO;
        for _ in 0..iters {
            let start = Instant::now();
            self.run(args);
            total += start.elapsed();
            self.run(&["undo"]);
        }
        total
    }
}

fn cli(c: &mut Criterion) {
    for (tasks, memories) in SEEDS {
        let sara = Sara::seeded(tasks, memories);
        let size = format!("{tasks}t_{memories}m");
        let mut group = c.benchmark_group("cli");
        group.sample_size(20);

        group.bench_with_input(BenchmarkId::new("list", &size), &sara, |b, s| {
            b.iter(|| s.run(&["list"]))
        });
        group.bench_with_input(BenchmarkId::new("recall_query", &size), &sara, |b, s| {
            b.iter(|| s.run(&["recall", "recall ranking"]))
        });
        group.bench_with_input(BenchmarkId::new("recall_tag", &size), &sara, |b, s| {
            b.iter(|| s.run(&["recall", "--tag", "bench"]))
        });
        group.bench_with_input(BenchmarkId::new("recall_file", &size), &sara, |b, s| {
            b.iter(|| s.run(&["recall", "--file", "src/module_1.rs"]))
        });
        group.bench_with_input(BenchmarkId::new("add", &size), &sara, |b, s| {
            b.iter_custom(|iters| {
                s.timed_then_undo(
                    iters,
                    &["add", "--yes", "-t", "bench", "Tune recall ranking"],
                )
            })
        });
        group.bench_with_input(BenchmarkId::new("begin", &size), &sara, |b, s| {
            b.iter_custom(|iters| {
                s.timed_then_undo(iters, &["begin", "-t", "bench", "Tune recall ranking"])
            })
        });
        group.finish();
    }
}

criterion_group!(benches, cli);
criterion_main!(benches);
