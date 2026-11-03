//! Shared download speed limiter (token bucket that all transfers draw from).

use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant, sleep};

/// A token bucket measured in bytes. Callers take bytes first and then wait for the debt to be
/// paid back, so concurrent transfers share the configured rate fairly.
pub struct Limiter {
    rate: AtomicU64,
    state: Mutex<(f64, Instant)>,
}

impl Limiter {
    /// `rate` is bytes per second, 0 = unlimited.
    pub fn new(rate: u64) -> Self {
        Limiter {
            rate: AtomicU64::new(rate),
            state: Mutex::new((0.0, Instant::now())),
        }
    }

    pub fn set_rate(&self, rate: u64) {
        self.rate.store(rate, Ordering::Relaxed);
    }

    pub fn rate(&self) -> u64 {
        self.rate.load(Ordering::Relaxed)
    }

    /// Waits until `n` more bytes may be consumed.
    pub async fn acquire(&self, n: usize) {
        let rate = self.rate();
        if rate == 0 {
            return;
        }
        let wait = {
            let mut st = self.state.lock().await;
            let now = Instant::now();
            let burst = (rate as f64 * 0.25).max(64.0 * 1024.0);
            st.0 = (st.0 + now.duration_since(st.1).as_secs_f64() * rate as f64).min(burst);
            st.1 = now;
            st.0 -= n as f64;
            if st.0 < 0.0 {
                Duration::from_secs_f64(-st.0 / rate as f64)
            } else {
                Duration::ZERO
            }
        };
        if !wait.is_zero() {
            sleep(wait).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn paces_to_the_configured_rate() {
        let l = Limiter::new(1 << 20); // 1 MiB/s
        let t0 = Instant::now();
        for _ in 0..48 {
            l.acquire(64 * 1024).await; // 3 MiB in total
        }
        let secs = t0.elapsed().as_secs_f64();
        assert!((2.5..3.2).contains(&secs), "3 MiB at 1 MiB/s took {secs}s");
    }

    #[tokio::test(start_paused = true)]
    async fn unlimited_and_rate_changes() {
        let l = Limiter::new(0);
        let t0 = Instant::now();
        for _ in 0..100 {
            l.acquire(1 << 20).await;
        }
        assert!(t0.elapsed() < Duration::from_millis(1));
        l.set_rate(100 * 1024);
        assert_eq!(l.rate(), 100 * 1024);
        let t1 = Instant::now();
        l.acquire(1 << 20).await; // 1 MiB at 100 KiB/s
        assert!(t1.elapsed().as_secs_f64() > 8.0);
    }
}
