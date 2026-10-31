// ──────────────────────────────────────────────────────────────────────
// mirrors/benchmark.rs — Real-Time Mirror Performance Benchmarking
// ──────────────────────────────────────────────────────────────────────

use std::net::{TcpStream, ToSocketAddrs};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use super::model::BenchmarkResult;

/// Progress message emitted during benchmark execution.
#[derive(Debug, Clone)]
pub struct BenchmarkProgress {
    pub total: usize,
    pub completed: usize,
    pub current_mirror: String,
    pub result: Option<BenchmarkResult>,
    pub finished: bool,
}

/// Resolve a raw mirrorlist template URL to a benchmarkable endpoint.
pub fn resolve_benchmark_url(template: &str, repo: Option<&str>, arch: Option<&str>) -> String {
    let r = repo.unwrap_or("core");
    let a = arch.unwrap_or("x86_64");

    let mut url = template.replace("$repo", r).replace("$arch", a);

    // If template ends with /, append repository database file to test real package throughput
    if url.ends_with('/') {
        url.push_str(&format!("{r}.db"));
    }

    url
}

/// Benchmark an individual mirror URL with bounded timeouts.
pub fn benchmark_single_mirror(raw_url: &str) -> BenchmarkResult {
    let resolved = resolve_benchmark_url(raw_url, None, None);

    if crate::sys::has_bin("curl") {
        benchmark_with_curl(raw_url, &resolved)
    } else {
        benchmark_with_tcp(raw_url, &resolved)
    }
}

/// Benchmark mirror using curl (extracts HTTP status, connect latency, throughput).
fn benchmark_with_curl(original_url: &str, target_url: &str) -> BenchmarkResult {
    let write_fmt = "code:%{http_code} connect:%{time_connect} start:%{time_starttransfer} speed:%{speed_download}";

    // Bounded timeouts: 3s connect timeout, 5s total max time, first 64KB
    let child = Command::new("curl")
        .args([
            "-s",
            "-L",
            "-o",
            "/dev/null",
            "-w",
            write_fmt,
            "--connect-timeout",
            "3",
            "--max-time",
            "5",
            "-r",
            "0-65535",
            target_url,
        ])
        .output();

    match child {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut http_code = 0u16;
            let mut connect_sec = 0.0f64;
            let mut speed_bytes = 0.0f64;

            for token in stdout.split_whitespace() {
                if let Some(val) = token.strip_prefix("code:") {
                    http_code = val.parse().unwrap_or(0);
                } else if let Some(val) = token.strip_prefix("connect:") {
                    connect_sec = val.parse().unwrap_or(0.0);
                } else if let Some(val) = token.strip_prefix("speed:") {
                    speed_bytes = val.parse().unwrap_or(0.0);
                }
            }

            let latency_ms = if connect_sec > 0.0 {
                Some((connect_sec * 1000.0) as u64)
            } else {
                None
            };

            let throughput_kbps = if speed_bytes > 0.0 {
                Some(speed_bytes / 1024.0)
            } else {
                None
            };

            let reachable = out.status.success() && (200..400).contains(&http_code);
            let error = if !reachable {
                if http_code > 0 {
                    Some(format!("HTTP {http_code}"))
                } else {
                    Some("Connection timeout or unreachable".to_string())
                }
            } else {
                None
            };

            BenchmarkResult {
                url: original_url.to_string(),
                reachable,
                http_status: if http_code > 0 { Some(http_code) } else { None },
                latency_ms,
                throughput_kbps,
                error,
            }
        }
        Err(e) => BenchmarkResult {
            url: original_url.to_string(),
            reachable: false,
            http_status: None,
            latency_ms: None,
            throughput_kbps: None,
            error: Some(e.to_string()),
        },
    }
}

/// Fallback TCP benchmark when curl is not present.
fn benchmark_with_tcp(original_url: &str, target_url: &str) -> BenchmarkResult {
    let clean = target_url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let host_port = clean.split('/').next().unwrap_or(clean);

    let (host, port) = if let Some((h, p)) = host_port.split_once(':') {
        (h, p.parse().unwrap_or(80))
    } else if target_url.starts_with("https://") {
        (host_port, 443)
    } else {
        (host_port, 80)
    };

    let start = Instant::now();
    let addr_str = format!("{host}:{port}");
    let reachable = match addr_str.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                TcpStream::connect_timeout(&addr, Duration::from_secs(3)).is_ok()
            } else {
                false
            }
        }
        Err(_) => false,
    };
    let elapsed = start.elapsed();

    BenchmarkResult {
        url: original_url.to_string(),
        reachable,
        http_status: if reachable { Some(200) } else { None },
        latency_ms: if reachable {
            Some(elapsed.as_millis() as u64)
        } else {
            None
        },
        throughput_kbps: None,
        error: if reachable {
            None
        } else {
            Some("TCP connection failed".to_string())
        },
    }
}

/// Spawn a background worker thread to benchmark mirror URLs with progress reporting and cancellation.
pub fn spawn_benchmark_job(
    urls: Vec<String>,
    cancel_flag: Arc<AtomicBool>,
) -> Receiver<BenchmarkProgress> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let total = urls.len();
        for (i, url) in urls.into_iter().enumerate() {
            if cancel_flag.load(Ordering::SeqCst) {
                let _ = tx.send(BenchmarkProgress {
                    total,
                    completed: i,
                    current_mirror: "Cancelled by user".to_string(),
                    result: None,
                    finished: true,
                });
                return;
            }

            let _ = tx.send(BenchmarkProgress {
                total,
                completed: i,
                current_mirror: url.clone(),
                result: None,
                finished: false,
            });

            let res = benchmark_single_mirror(&url);

            let _ = tx.send(BenchmarkProgress {
                total,
                completed: i + 1,
                current_mirror: url,
                result: Some(res),
                finished: false,
            });
        }

        let _ = tx.send(BenchmarkProgress {
            total,
            completed: total,
            current_mirror: "Finished".to_string(),
            result: None,
            finished: true,
        });
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_benchmark_url() {
        let t1 = "https://mirror.rackspace.com/archlinux/$repo/os/$arch";
        let r1 = resolve_benchmark_url(t1, Some("core"), Some("x86_64"));
        assert_eq!(r1, "https://mirror.rackspace.com/archlinux/core/os/x86_64");

        let t2 = "https://mirror.cachyos.org/repo/$arch/$repo/";
        let r2 = resolve_benchmark_url(t2, Some("cachyos"), Some("x86_64"));
        assert_eq!(
            r2,
            "https://mirror.cachyos.org/repo/x86_64/cachyos/cachyos.db"
        );
    }

    #[test]
    fn test_benchmark_job_cancellation() {
        let cancel = Arc::new(AtomicBool::new(true)); // Cancel immediately
        let urls = vec![
            "https://mirror1.invalid".to_string(),
            "https://mirror2.invalid".to_string(),
        ];
        let rx = spawn_benchmark_job(urls, cancel);

        let mut received_cancel = false;
        while let Ok(msg) = rx.recv_timeout(Duration::from_secs(2)) {
            if msg.current_mirror.contains("Cancelled") {
                received_cancel = true;
                break;
            }
        }
        assert!(received_cancel);
    }
}
