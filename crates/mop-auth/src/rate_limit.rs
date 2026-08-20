use mop_core::error::AppError;
use std::collections::HashMap;
use std::hash::Hash;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct IpRateLimiter {
    max_requests: usize,
    window: Duration,
    state: Arc<Mutex<HashMap<IpAddr, Vec<Instant>>>>,
}

impl IpRateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Default auth rate limiter: 5 requests per 60 seconds (SPEC.md §8.1 / §19.10)
    pub fn new_auth_limiter() -> Self {
        Self::new(5, Duration::from_secs(60))
    }

    pub async fn check(&self, ip: IpAddr) -> Result<(), AppError> {
        let mut map = self.state.lock().await;
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);

        let entries = map.entry(ip).or_default();
        entries.retain(|&t| t > cutoff);

        if entries.len() >= self.max_requests {
            return Err(AppError::RateLimitExceeded);
        }

        entries.push(now);
        Ok(())
    }

    /// Reset counter for a specific IP (e.g. upon successful authentication)
    pub async fn reset_ip(&self, ip: IpAddr) {
        let mut map = self.state.lock().await;
        map.remove(&ip);
    }

    pub async fn reset_all(&self) {
        let mut map = self.state.lock().await;
        map.clear();
    }
}

/// Generic key-based sliding window rate limiter (e.g. per-user action rate limiter)
#[derive(Clone)]
pub struct KeyRateLimiter<K: Hash + Eq + Send + Clone + 'static> {
    max_requests: usize,
    window: Duration,
    state: Arc<Mutex<HashMap<K, Vec<Instant>>>>,
}

impl<K: Hash + Eq + Send + Clone + 'static> KeyRateLimiter<K> {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self {
            max_requests,
            window,
            state: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Action rate limiter: 10 requests per 60 seconds per user (SPEC.md §19.10)
    pub fn new_action_limiter() -> Self {
        Self::new(10, Duration::from_secs(60))
    }

    pub async fn check(&self, key: K) -> Result<(), AppError> {
        let mut map = self.state.lock().await;
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);

        let entries = map.entry(key).or_default();
        entries.retain(|&t| t > cutoff);

        if entries.len() >= self.max_requests {
            return Err(AppError::RateLimitExceeded);
        }

        entries.push(now);
        Ok(())
    }

    pub async fn reset_key(&self, key: &K) {
        let mut map = self.state.lock().await;
        map.remove(key);
    }

    pub async fn reset_all(&self) {
        let mut map = self.state.lock().await;
        map.clear();
    }
}
