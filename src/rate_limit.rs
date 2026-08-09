use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_MAX_KEYS: usize = 10_000;
const INVALID_IP_KEY: &str = "<invalid-client-ip>";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RateLimitKey(String);

impl RateLimitKey {
    pub fn from_ip(ip: IpAddr) -> Self {
        Self(ip.to_string())
    }

    pub fn parse_ip(value: &str) -> Option<Self> {
        value.parse::<IpAddr>().ok().map(Self::from_ip)
    }

    fn invalid_ip() -> Self {
        Self(INVALID_IP_KEY.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Process-local sliding-window limiter with a hard bound on distinct keys.
#[derive(Clone)]
pub struct RateLimiter {
    entries: Arc<DashMap<String, Vec<Instant>>>,
    max_requests: usize,
    max_keys: usize,
    window: Duration,
    mutation_lock: Arc<std::sync::Mutex<()>>,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window: Duration) -> Self {
        Self::with_capacity(
            max_requests,
            window,
            positive_env_usize("MONOIZE_AUTH_RATE_LIMIT_MAX_KEYS", DEFAULT_MAX_KEYS),
        )
    }

    pub fn with_capacity(max_requests: usize, window: Duration, max_keys: usize) -> Self {
        Self {
            entries: Arc::new(DashMap::new()),
            max_requests: max_requests.max(1),
            max_keys: max_keys.max(1),
            window,
            mutation_lock: Arc::new(std::sync::Mutex::new(())),
        }
    }

    /// Compatibility API for existing callers. Invalid input shares one key.
    pub fn check(&self, key: &str) -> bool {
        let key = RateLimitKey::parse_ip(key).unwrap_or_else(RateLimitKey::invalid_ip);
        self.check_key(&key)
    }

    pub fn check_ip(&self, ip: IpAddr) -> bool {
        self.check_key(&RateLimitKey::from_ip(ip))
    }

    pub fn check_key(&self, key: &RateLimitKey) -> bool {
        let _guard = self
            .mutation_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);

        if !self.entries.contains_key(key.as_str()) && self.entries.len() >= self.max_keys {
            return false;
        }

        let mut entry = self.entries.entry(key.as_str().to_string()).or_default();
        entry.retain(|&timestamp| timestamp > cutoff);
        if entry.len() >= self.max_requests {
            return false;
        }
        entry.push(now);
        true
    }

    pub fn cleanup(&self) {
        let _guard = self
            .mutation_lock
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let now = Instant::now();
        self.cleanup_at_locked(now.checked_sub(self.window).unwrap_or(now));
    }

    fn cleanup_at_locked(&self, cutoff: Instant) {
        self.entries.retain(|_, timestamps| {
            timestamps.retain(|&timestamp| timestamp > cutoff);
            !timestamps.is_empty()
        });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

fn positive_env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_ipv6_forms_share_one_bucket() {
        let limiter = RateLimiter::with_capacity(1, Duration::from_secs(60), 4);
        assert!(limiter.check("2001:0db8:0:0:0:0:0:1"));
        assert!(!limiter.check("2001:db8::1"));
        assert_eq!(limiter.len(), 1);
    }

    #[test]
    fn unseen_key_is_rejected_at_capacity() {
        let limiter = RateLimiter::with_capacity(2, Duration::from_secs(60), 1);
        assert!(limiter.check("192.0.2.1"));
        assert!(!limiter.check("192.0.2.2"));
        assert_eq!(limiter.len(), 1);
    }

    #[test]
    fn invalid_strings_cannot_create_arbitrary_keys() {
        let limiter = RateLimiter::with_capacity(1, Duration::from_secs(60), 4);
        assert!(limiter.check("attacker-a"));
        assert!(!limiter.check("attacker-b"));
        assert_eq!(limiter.len(), 1);
    }

    #[test]
    fn concurrent_unseen_keys_cannot_exceed_capacity() {
        let limiter = RateLimiter::with_capacity(1, Duration::from_secs(60), 4);
        let barrier = Arc::new(std::sync::Barrier::new(17));
        let mut handles = Vec::new();
        for index in 0..16 {
            let limiter = limiter.clone();
            let barrier = barrier.clone();
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                limiter.check(&format!("192.0.2.{}", index + 1))
            }));
        }
        barrier.wait();
        let accepted = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .filter(|accepted| *accepted)
            .count();
        assert_eq!(accepted, 4);
        assert_eq!(limiter.len(), 4);
    }

    #[test]
    fn expired_capacity_is_reclaimed_only_by_explicit_cleanup() {
        let limiter = RateLimiter::with_capacity(1, Duration::from_millis(1), 1);
        assert!(limiter.check("192.0.2.1"));
        std::thread::sleep(Duration::from_millis(5));
        assert!(!limiter.check("192.0.2.2"));
        assert_eq!(limiter.len(), 1);
        limiter.cleanup();
        assert!(limiter.check("192.0.2.2"));
    }
}
