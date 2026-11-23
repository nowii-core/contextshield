#[derive(Debug, Clone)]
pub struct TelemetrySnapshot {
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub blocked_requests: u64,
    pub redacted_secrets: u64,
    pub cache_entries: usize,
    pub p50_us: u128,
    pub p95_us: u128,
    pub p99_us: u128,
}

pub struct GatewayTelemetry {
    total_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    blocked_requests: u64,
    redacted_secrets: u64,
    latencies_us: Vec<u128>,
}

impl Default for GatewayTelemetry {
    fn default() -> Self {
        Self::new()
    }
}

impl GatewayTelemetry {
    pub fn new() -> Self {
        Self {
            total_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            blocked_requests: 0,
            redacted_secrets: 0,
            latencies_us: Vec::with_capacity(1024),
        }
    }

    pub fn record_redactions(&mut self, count: u64) {
        self.redacted_secrets += count;
    }

    pub fn record_request(&mut self, cache_hit: bool, blocked: bool, latency_us: u128) {
        self.total_requests += 1;
        if blocked {
            self.blocked_requests += 1;
        } else if cache_hit {
            self.cache_hits += 1;
        } else {
            self.cache_misses += 1;
        }
        if self.latencies_us.len() >= 10_000 {
            self.latencies_us.remove(0);
        }
        self.latencies_us.push(latency_us);
    }

    pub fn snapshot(&self, cache_entries: usize) -> TelemetrySnapshot {
        let mut sorted = self.latencies_us.clone();
        sorted.sort_unstable();
        TelemetrySnapshot {
            total_requests: self.total_requests,
            cache_hits: self.cache_hits,
            cache_misses: self.cache_misses,
            blocked_requests: self.blocked_requests,
            redacted_secrets: self.redacted_secrets,
            cache_entries,
            p50_us: percentile(&sorted, 50.0),
            p95_us: percentile(&sorted, 95.0),
            p99_us: percentile(&sorted, 99.0),
        }
    }
}

fn percentile(sorted: &[u128], pct: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = (((pct / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize).min(sorted.len() - 1);
    sorted[idx]
}
