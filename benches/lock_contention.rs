use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tokio::sync::Mutex;

// ============================================================================
// Helper: distribute `total` units across `threads` such that each thread
// does at most `ceil(total / threads)` units, and the total is preserved.
// ============================================================================
fn distribute(total: u64, threads: usize) -> impl Iterator<Item = u64> {
    let per = total / threads as u64;
    let rem = total % threads as u64;
    (0..threads as u64).map(move |i| per + if i < rem { 1 } else { 0 })
}

// ============================================================================
// std::sync::RwLock — models REGISTRY in metadata.rs
// ============================================================================

fn rwlock_contention(c: &mut Criterion) {
    let mut group = c.benchmark_group("std_rwlock");
    group.measurement_time(Duration::from_secs(10));

    // uncontended baseline
    let lock = RwLock::new(0u64);
    group.bench_function("read_baseline", |b| {
        b.iter(|| {
            let guard = lock.read().unwrap();
            std::hint::black_box(*guard);
        });
    });
    group.bench_function("write_baseline", |b| {
        b.iter(|| {
            let mut guard = lock.write().unwrap();
            *guard = std::hint::black_box(guard.wrapping_add(1));
        });
    });

    // multi-threaded read
    for &threads in &[2, 4, 8, 16, 32] {
        let lock = Arc::new(RwLock::new(0u64));
        group.throughput(Throughput::Elements(threads as u64));
        group.bench_with_input(BenchmarkId::new("read", threads), &threads, |b, &t| {
            let lock = Arc::clone(&lock);
            b.iter_custom(move |iters| {
                let start = Instant::now();
                thread::scope(|s| {
                    for n in distribute(iters, t) {
                        let lock = Arc::clone(&lock);
                        s.spawn(move || {
                            for _ in 0..n {
                                let guard = lock.read().unwrap();
                                std::hint::black_box(*guard);
                            }
                        });
                    }
                });
                start.elapsed()
            });
        });
    }

    // multi-threaded write
    for &threads in &[2, 4, 8] {
        let lock = Arc::new(RwLock::new(0u64));
        group.throughput(Throughput::Elements(threads as u64));
        group.bench_with_input(BenchmarkId::new("write", threads), &threads, |b, &t| {
            let lock = Arc::clone(&lock);
            b.iter_custom(move |iters| {
                let start = Instant::now();
                thread::scope(|s| {
                    for n in distribute(iters, t) {
                        let lock = Arc::clone(&lock);
                        s.spawn(move || {
                            for _ in 0..n {
                                let mut guard = lock.write().unwrap();
                                *guard = std::hint::black_box(guard.wrapping_add(1));
                            }
                        });
                    }
                });
                start.elapsed()
            });
        });
    }

    // mixed 90/10 read/write
    for &threads in &[4, 8, 16] {
        let lock = Arc::new(RwLock::new(0u64));
        let readers = (threads as f64 * 0.9).round() as usize;
        group.throughput(Throughput::Elements(threads as u64));
        group.bench_with_input(BenchmarkId::new("mixed_90_10", threads), &threads, |b, &t| {
            let lock = Arc::clone(&lock);
            b.iter_custom(move |iters| {
                let start = Instant::now();
                thread::scope(|s| {
                    for (i, n) in distribute(iters, t).enumerate() {
                        let lock = Arc::clone(&lock);
                        let is_writer = i >= readers;
                        s.spawn(move || {
                            for _ in 0..n {
                                if is_writer {
                                    let mut guard = lock.write().unwrap();
                                    *guard = std::hint::black_box(guard.wrapping_add(1));
                                } else {
                                    let guard = lock.read().unwrap();
                                    std::hint::black_box(*guard);
                                }
                            }
                        });
                    }
                });
                start.elapsed()
            });
        });
    }

    group.finish();
}

// ============================================================================
// Real-world REGISTRY benchmark — exercises the actual global lock used on
// every hot-path query in debug builds.
// ============================================================================
fn registry_contention(c: &mut Criterion) {
    use bridge_rs::engine::metadata::{self, ColumnMetadata, EntityMapping};
    use std::collections::HashMap;

    // Populate with 100 entities × 10 columns — mirrors real usage.
    {
        let mut reg = metadata::REGISTRY.write().unwrap();
        reg.mappings.clear();
        for i in 0..100 {
            let mut columns = HashMap::new();
            for j in 0..10 {
                columns.insert(
                    format!("col_{j}"),
                    ColumnMetadata {
                        name: format!("col_{j}"),
                        data_type: match j % 3 {
                            0 => "TEXT".into(),
                            1 => "BIGINT".into(),
                            _ => "BOOLEAN".into(),
                        },
                        is_nullable: j % 2 == 0,
                        is_primary_key: j == 0,
                    },
                );
            }
            reg.mappings.insert(
                format!("entity_{i}"),
                EntityMapping {
                    table_name: format!("entity_{i}"),
                    columns,
                },
            );
        }
    }

    let mut group = c.benchmark_group("registry");
    group.measurement_time(Duration::from_secs(10));

    for &threads in &[1, 2, 4, 8, 16] {
        group.throughput(Throughput::Elements(threads as u64));
        group.bench_with_input(BenchmarkId::new("read", threads), &threads, |b, &t| {
            b.iter_custom(move |iters| {
                let start = Instant::now();
                thread::scope(|s| {
                    for n in distribute(iters, t) {
                        s.spawn(move || {
                            for _ in 0..n {
                                let guard = metadata::REGISTRY.read().unwrap();
                                let _entry = guard.mappings.get("entity_5");
                                std::hint::black_box(());
                            }
                        });
                    }
                });
                start.elapsed()
            });
        });
    }
    group.finish();
}

// ============================================================================
// tokio::sync::Mutex — models transaction Mutex in db.rs hot paths
// ============================================================================
fn tokio_mutex_contention(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("tokio_mutex");
    group.measurement_time(Duration::from_secs(10));

    // uncontended baseline
    let lock = Arc::new(Mutex::new(0u64));
    group.bench_function("uncontended", |b| {
        b.to_async(&rt).iter(|| {
            let lock = Arc::clone(&lock);
            async move {
                let mut guard = lock.lock().await;
                *guard = std::hint::black_box(guard.wrapping_add(1));
            }
        });
    });

    // multi-task contended
    for &tasks in &[2, 4, 8, 16, 32, 64] {
        let lock = Arc::new(Mutex::new(0u64));
        group.throughput(Throughput::Elements(tasks as u64));
        group.bench_with_input(BenchmarkId::new("contended", tasks), &tasks, |b, &t| {
            let lock = Arc::clone(&lock);
            b.iter_custom(move |iters| {
                let lock = Arc::clone(&lock);
                let rt = tokio::runtime::Runtime::new().unwrap();
                let start = Instant::now();
                rt.block_on(async {
                    let mut handles = Vec::with_capacity(t);
                    for n in distribute(iters, t) {
                        let lock = Arc::clone(&lock);
                        handles.push(tokio::spawn(async move {
                            for _ in 0..n {
                                let mut guard = lock.lock().await;
                                *guard = std::hint::black_box(guard.wrapping_add(1));
                            }
                        }));
                    }
                    for h in handles {
                        let _ = h.await;
                    }
                });
                start.elapsed()
            });
        });
    }
    group.finish();
}

criterion_group!(benches, rwlock_contention, registry_contention, tokio_mutex_contention);
criterion_main!(benches);
