//! Headless CLI hardware benchmarks (`duke-launcher.exe --bench`).
//!
//! Bypasses Tauri and WebView2 initialization (`0 MB` renderer RAM) and exercises
//! the four core Duke Suite engines:
//! 1. `duke-shm` lock-free ring buffer (`100,000` atomic frames)
//! 2. `.dkcat` zero-copy `memmap2` 30,000-record mount + trigram/bitflag query
//! 3. `BLAKE3` 64 KiB streaming content hashing
//! 4. `FastCDC v2020` Gear-hash content-defined chunking (16–64 KiB)

use crate::catalog_mmap::{serialize_dkcat, CatalogQueryParams, MappedCatalog};
use crate::delta_patch::{FASTCDC_AVG_SIZE, FASTCDC_MAX_SIZE, FASTCDC_MIN_SIZE};
use crate::{CatalogCacheFile, CatalogGameEntry};
use duke_core::shm_bus::{DukeShmBus, ShmEventFrame, ShmEventType, ShmSourceApp};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const SHM_FRAME_COUNT: usize = 100_000;
const SHM_BATCH_SIZE: usize = 512;
const DKCAT_RECORD_COUNT: usize = 30_000;
const DKCAT_QUERY_ITERS: usize = 100;
const BLAKE3_BUF_BYTES: usize = 4 * 1024 * 1024;
const BLAKE3_PASSES: usize = 32; // 128 MiB streamed in 64 KiB chunks
const BLAKE3_CHUNK_BYTES: usize = 64 * 1024;
const FASTCDC_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchReport {
    pub shm_frames: usize,
    pub shm_ops_per_sec: f64,
    pub shm_ns_per_op: f64,
    pub dkcat_record_count: u32,
    pub dkcat_mount_us: f64,
    pub dkcat_query_us: f64,
    pub dkcat_query_hits: usize,
    pub blake3_bytes: usize,
    pub blake3_gb_per_sec: f64,
    pub blake3_elapsed_ms: f64,
    pub fastcdc_bytes: usize,
    pub fastcdc_chunks: usize,
    pub fastcdc_mb_per_sec: f64,
    pub fastcdc_elapsed_ms: f64,
    pub measured_total_ms: f64,
    pub wall_total_ms: f64,
}

struct TempBenchDir(PathBuf);

impl TempBenchDir {
    fn create(suffix: &str) -> Result<Self, String> {
        let nanos = unix_nanos();
        let dir = std::env::temp_dir().join(format!(
            "duke-launcher-bench-{}-{suffix}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Failed to create benchmark temp dir {}: {e}", dir.display()))?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBenchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(windows)]
fn ensure_console_stdout_attached() {
    const STD_OUTPUT_HANDLE: u32 = (-11i32) as u32;
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    const INVALID_HANDLE_VALUE: isize = -1;
    const CP_UTF8: u32 = 65001;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n_std_handle: u32) -> isize;
        fn AttachConsole(dw_process_id: u32) -> i32;
        fn SetConsoleOutputCP(w_code_page_id: u32) -> i32;
    }

    // Bind parent terminal only when no pipe/file stdout handle is inherited
    // (safe under both redirected CI captures and GUI-subsystem CLI launches).
    let stdout_handle = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if stdout_handle == 0 || stdout_handle == INVALID_HANDLE_VALUE {
        unsafe {
            let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    unsafe {
        let _ = SetConsoleOutputCP(CP_UTF8);
    }
}

#[cfg(not(windows))]
fn ensure_console_stdout_attached() {}

/// Runs `--bench`, prints the Markdown benchmark table to stdout, and exits cleanly.
pub fn run_cli_benchmark() -> Result<(), String> {
    ensure_console_stdout_attached();
    let report = run_benchmarks()?;
    print!("{}", render_benchmark_table(&report));
    Ok(())
}

/// Executes all 4 headless core benchmarks and returns structured timings.
pub fn run_benchmarks() -> Result<BenchReport, String> {
    let wall_started = Instant::now();

    let (shm_ops_per_sec, shm_ns_per_op, shm_elapsed_ms) = bench_shm_ring()?;
    let (dkcat_record_count, dkcat_mount_us, dkcat_query_us, dkcat_query_hits) =
        bench_dkcat_mmap()?;
    let (blake3_bytes, blake3_gb_per_sec, blake3_elapsed_ms) = bench_blake3_streaming();
    let (fastcdc_bytes, fastcdc_chunks, fastcdc_mb_per_sec, fastcdc_elapsed_ms) =
        bench_fastcdc_chunking();

    let measured_total_ms = shm_elapsed_ms
        + (dkcat_mount_us + dkcat_query_us * DKCAT_QUERY_ITERS as f64) / 1000.0
        + blake3_elapsed_ms
        + fastcdc_elapsed_ms;
    let wall_total_ms = wall_started.elapsed().as_secs_f64() * 1000.0;

    Ok(BenchReport {
        shm_frames: SHM_FRAME_COUNT,
        shm_ops_per_sec,
        shm_ns_per_op,
        dkcat_record_count,
        dkcat_mount_us,
        dkcat_query_us,
        dkcat_query_hits,
        blake3_bytes,
        blake3_gb_per_sec,
        blake3_elapsed_ms,
        fastcdc_bytes,
        fastcdc_chunks,
        fastcdc_mb_per_sec,
        fastcdc_elapsed_ms,
        measured_total_ms,
        wall_total_ms,
    })
}

fn bench_shm_ring() -> Result<(f64, f64, f64), String> {
    let mapping_name = format!(
        "Local\\duke-suite-bench-{}-{}",
        std::process::id(),
        unix_nanos()
    );
    let mut bus = DukeShmBus::open_or_create_named(&mapping_name)?;
    let start_seq = bus.write_cursor();
    bus.seek_sequence(start_seq);

    let pid = std::process::id();
    let template = ShmEventFrame::new(
        ShmEventType::TelemetryTick,
        ShmSourceApp::DukeLauncher,
        pid,
        1_700_000_000_000,
        0x0044_0063,
        16_384,
        [0u8; 24],
    );

    let mut received = 0usize;
    let mut published = 0usize;

    let started = Instant::now();
    while published < SHM_FRAME_COUNT {
        let batch = (SHM_FRAME_COUNT - published).min(SHM_BATCH_SIZE);
        for _ in 0..batch {
            let seq = bus.publish_frame(template);
            black_box(seq);
        }
        published += batch;
        while let Some(frame) = bus.try_recv() {
            black_box(frame);
            received += 1;
        }
    }
    let elapsed = started.elapsed();

    if received != SHM_FRAME_COUNT {
        return Err(format!(
            "SHM benchmark frame mismatch: expected {SHM_FRAME_COUNT}, received {received} (lagged {})",
            bus.lagged_events()
        ));
    }

    let elapsed_secs = elapsed.as_secs_f64().max(1e-9);
    let ops_per_sec = SHM_FRAME_COUNT as f64 / elapsed_secs;
    let ns_per_op = (elapsed.as_secs_f64() * 1_000_000_000.0) / SHM_FRAME_COUNT as f64;
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    Ok((ops_per_sec, ns_per_op, elapsed_ms))
}

fn build_synthetic_dkcat_entries(count: usize) -> Vec<CatalogGameEntry> {
    const GENRES: [&str; 5] = ["Action", "RPG", "Indie", "Strategy", "Simulation"];
    const TAGS: [&str; 6] = [
        "Singleplayer",
        "Co-op",
        "Atmospheric",
        "Controller",
        "Open World",
        "Sci-Fi",
    ];

    let mut entries = Vec::with_capacity(count);
    for idx in 0..count {
        let is_target_match = idx % 500 == 0;
        let is_free = !is_target_match && idx % 10 == 0;
        let name = if is_target_match {
            format!("Elden Benchmark Ring {:05}", idx)
        } else {
            format!("Duke Catalog Benchmark Title {:05}", idx)
        };
        entries.push(CatalogGameEntry {
            app_id: 200_000 + idx as u32,
            name,
            price_cents: if is_free { 0 } else { 1999 + ((idx % 4) * 500) as u32 },
            initial_price_cents: if is_free { 0 } else { 2999 },
            discount_percent: if is_free { 0 } else { 33 },
            price_formatted: if is_free {
                "Free".to_string()
            } else {
                "$19.99".to_string()
            },
            is_free,
            ccu: (100 + (idx % 9_000)) as u64,
            positive: (500 + (idx % 5_000)) as u64,
            negative: (40 + (idx % 300)) as u64,
            review_pct: (78 + (idx % 20)) as f64,
            is_singleplayer: is_target_match || idx % 2 == 0,
            is_coop: is_target_match || idx % 3 == 0,
            is_multiplayer: idx % 4 == 0,
            is_online_only_pvp: !is_target_match && idx % 7 == 0,
            genres: vec![GENRES[idx % GENRES.len()].to_string()],
            tags: vec![TAGS[idx % TAGS.len()].to_string()],
            value_score: 7.5 + (idx % 25) as f64 * 0.1,
            updated_at: 1_700_000_000,
            ..Default::default()
        });
    }
    entries
}

fn bench_dkcat_mmap() -> Result<(u32, f64, f64, usize), String> {
    let temp_dir = TempBenchDir::create("dkcat")?;
    let dkcat_path = temp_dir.path().join("bench_catalog.dkcat");

    let catalog = CatalogCacheFile {
        fetched_at: 1_700_000_000,
        fetched_pages: vec![0],
        entries: build_synthetic_dkcat_entries(DKCAT_RECORD_COUNT),
    };
    let payload = serialize_dkcat(&catalog)?;
    std::fs::write(&dkcat_path, &payload)
        .map_err(|e| format!("Failed to write benchmark .dkcat archive: {e}"))?;
    drop(payload);
    drop(catalog);

    let mount_started = Instant::now();
    let mapped = MappedCatalog::open(&dkcat_path)?;
    let mount_elapsed_us = mount_started.elapsed().as_secs_f64() * 1_000_000.0;
    let stats = mapped.stats();
    let mount_us = mount_elapsed_us.max(stats.map_time_us as f64);

    let query_params = CatalogQueryParams {
        query: "elden ring".to_string(),
        sort_by: "value_score".to_string(),
        paid_only: true,
        singleplayer: true,
        coop: true,
        exclude_pvp: true,
        hide_installed: false,
        installed_app_ids: Vec::new(),
        installed_titles_norm: Vec::new(),
        min_price_cents: 0,
        min_ccu: 0,
        genres: Vec::new(),
        offset: 0,
        limit: 60,
        ..CatalogQueryParams::default()
    };

    // Warm once to fault trigram index pages before timing steady-state queries.
    let warmup = mapped.query(&query_params);
    if warmup.total_matches == 0 {
        return Err(".dkcat benchmark query matched 0 records unexpectedly".to_string());
    }

    let query_started = Instant::now();
    let mut last_matches = warmup.total_matches;
    for _ in 0..DKCAT_QUERY_ITERS {
        let page = mapped.query(&query_params);
        last_matches = page.total_matches;
        black_box(page);
    }
    let query_us = (query_started.elapsed().as_secs_f64() * 1_000_000.0) / DKCAT_QUERY_ITERS as f64;

    Ok((stats.record_count, mount_us, query_us, last_matches))
}

fn fill_deterministic_bytes(buf: &mut [u8], mut seed: u64) {
    let mut offset = 0usize;
    while offset + 8 <= buf.len() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        buf[offset..offset + 8].copy_from_slice(&seed.to_le_bytes());
        offset += 8;
    }
    for byte in &mut buf[offset..] {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *byte = (seed >> 56) as u8;
    }
}

fn bench_blake3_streaming() -> (usize, f64, f64) {
    let mut buf = vec![0u8; BLAKE3_BUF_BYTES];
    fill_deterministic_bytes(&mut buf, 0xD0CE_5EED_424C_414B);

    let total_bytes = BLAKE3_BUF_BYTES * BLAKE3_PASSES;
    let started = Instant::now();
    let mut hasher = blake3::Hasher::new();
    for _ in 0..BLAKE3_PASSES {
        for chunk in buf.chunks(BLAKE3_CHUNK_BYTES) {
            hasher.update(chunk);
        }
    }
    let digest = hasher.finalize();
    black_box(digest);
    let elapsed = started.elapsed();

    let elapsed_secs = elapsed.as_secs_f64().max(1e-9);
    let gb_per_sec = (total_bytes as f64 / (1024.0 * 1024.0 * 1024.0)) / elapsed_secs;
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    (total_bytes, gb_per_sec, elapsed_ms)
}

fn bench_fastcdc_chunking() -> (usize, usize, f64, f64) {
    let mut buf = vec![0u8; FASTCDC_PAYLOAD_BYTES];
    fill_deterministic_bytes(&mut buf, 0xFA57_CDC0_2020_D0CE);

    let started = Instant::now();
    let chunker = fastcdc::v2020::StreamCDC::new(
        buf.as_slice(),
        FASTCDC_MIN_SIZE,
        FASTCDC_AVG_SIZE,
        FASTCDC_MAX_SIZE,
    );
    let mut chunk_count = 0usize;
    let mut chunked_bytes = 0usize;
    for chunk in chunker.flatten() {
        chunk_count += 1;
        chunked_bytes += chunk.length;
        black_box(chunk.hash);
    }
    black_box(chunked_bytes);
    let elapsed = started.elapsed();

    let elapsed_secs = elapsed.as_secs_f64().max(1e-9);
    let mb_per_sec = (chunked_bytes as f64 / (1024.0 * 1024.0)) / elapsed_secs;
    let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
    (chunked_bytes, chunk_count, mb_per_sec, elapsed_ms)
}

fn format_ops_per_sec(ops_per_sec: f64) -> String {
    if ops_per_sec >= 1_000_000.0 {
        format!("{:.2}M ops/s", ops_per_sec / 1_000_000.0)
    } else if ops_per_sec >= 1_000.0 {
        format!("{:.1}K ops/s", ops_per_sec / 1_000.0)
    } else {
        format!("{:.0} ops/s", ops_per_sec)
    }
}

pub fn render_benchmark_table(report: &BenchReport) -> String {
    let mut out = String::new();
    out.push_str("# Duke Suite Hardware Benchmarks (`duke-launcher.exe --bench`)\n\n");
    out.push_str(&format!(
        "| {:<24} | {:<39} | {:>15} | {:>18} |\n",
        "Subsystem", "Workload", "Throughput", "Latency / Detail"
    ));
    out.push_str(&format!("|{:-<26}|{:-<41}|{:-<17}|{:-<20}|\n", "", "", "", ""));
    out.push_str(&format!(
        "| {:<24} | {:<39} | {:>15} | {:>18} |\n",
        "duke-shm (Lock-Free)",
        format!("{} atomic ring frames", report.shm_frames),
        format_ops_per_sec(report.shm_ops_per_sec),
        format!("{:.1} ns/op", report.shm_ns_per_op),
    ));
    out.push_str(&format!(
        "| {:<24} | {:<39} | {:>15} | {:>18} |\n",
        ".dkcat (memmap2)",
        format!("{}-record zero-copy mount", report.dkcat_record_count),
        format!("{:.2}M rec/s", (report.dkcat_record_count as f64) / report.dkcat_mount_us.max(0.1)),
        format!("{:.1} µs mount", report.dkcat_mount_us),
    ));
    out.push_str(&format!(
        "| {:<24} | {:<39} | {:>15} | {:>18} |\n",
        ".dkcat (trigram+flags)",
        format!("Bitflag + trigram query ({} hits)", report.dkcat_query_hits),
        format_ops_per_sec(1_000_000.0 / report.dkcat_query_us.max(0.1)),
        format!("{:.1} µs query", report.dkcat_query_us),
    ));
    out.push_str(&format!(
        "| {:<24} | {:<39} | {:>15} | {:>18} |\n",
        "BLAKE3 (64 KiB stream)",
        format!("{} MiB streamed digest", report.blake3_bytes / (1024 * 1024)),
        format!("{:.2} GB/s", report.blake3_gb_per_sec),
        format!("{:.2} ms", report.blake3_elapsed_ms),
    ));
    out.push_str(&format!(
        "| {:<24} | {:<39} | {:>15} | {:>18} |\n",
        "FastCDC v2020 (Gear)",
        format!(
            "{} MiB -> {} chunks (16-64 KiB)",
            report.fastcdc_bytes / (1024 * 1024),
            report.fastcdc_chunks
        ),
        format!("{:.1} MB/s", report.fastcdc_mb_per_sec),
        format!("{:.2} ms", report.fastcdc_elapsed_ms),
    ));
    out.push_str(&format!(
        "\nMeasured Compute: {:.1} ms | End-to-End (incl. fixture build): {:.2} s | GUI RAM: 0 MB\n",
        report.measured_total_ms,
        report.wall_total_ms / 1000.0
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_benchmarks_run_and_render_all_subsystems() {
        let report = run_benchmarks().expect("headless --bench suite should succeed");
        assert_eq!(report.shm_frames, SHM_FRAME_COUNT);
        assert!(report.shm_ops_per_sec > 0.0);
        assert!(report.shm_ns_per_op > 0.0);
        assert_eq!(report.dkcat_record_count, DKCAT_RECORD_COUNT as u32);
        assert!(report.dkcat_mount_us > 0.0);
        assert!(report.dkcat_query_us > 0.0);
        assert!(report.dkcat_query_hits >= 50);
        assert!(report.blake3_gb_per_sec > 0.0);
        assert!(report.fastcdc_mb_per_sec > 0.0);
        assert!(report.fastcdc_chunks > 100);

        let table = render_benchmark_table(&report);
        assert!(table.contains("duke-shm"));
        assert!(table.contains(".dkcat"));
        assert!(table.contains("BLAKE3"));
        assert!(table.contains("FastCDC v2020"));
        assert!(table.contains("GUI RAM: 0 MB"));
    }
}
