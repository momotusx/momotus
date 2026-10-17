use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

/// Rolling health for one addon: EWMA latency of successful calls and an EWMA
/// failure rate, both with alpha 0.2.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AddonHealth {
    pub latency_ms: f64,
    pub failure_rate: f64,
    pub samples: u32,
}

const ALPHA: f64 = 0.2;

impl AddonHealth {
    fn record(&mut self, latency: Duration, ok: bool) {
        let failure = if ok { 0.0 } else { 1.0 };
        if self.samples == 0 {
            self.failure_rate = failure;
            if ok {
                self.latency_ms = latency.as_secs_f64() * 1000.0;
            }
        } else {
            self.failure_rate += ALPHA * (failure - self.failure_rate);
            if ok {
                let ms = latency.as_secs_f64() * 1000.0;
                self.latency_ms += ALPHA * (ms - self.latency_ms);
            }
        }
        self.samples = self.samples.saturating_add(1);
    }
}

#[derive(Debug, Default)]
pub struct HealthBook {
    by_addon: Mutex<HashMap<String, AddonHealth>>,
}

impl HealthBook {
    pub fn record(&self, addon_id: &str, latency: Duration, ok: bool) {
        let mut map = self.by_addon.lock().unwrap_or_else(|p| p.into_inner());
        map.entry(addon_id.to_owned())
            .or_default()
            .record(latency, ok);
    }

    pub fn get(&self, addon_id: &str) -> Option<AddonHealth> {
        let map = self.by_addon.lock().unwrap_or_else(|p| p.into_inner());
        map.get(addon_id).copied()
    }

    pub fn snapshot(&self) -> HashMap<String, AddonHealth> {
        self.by_addon
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_moves_toward_recent_samples() {
        let book = HealthBook::default();
        book.record("a", Duration::from_millis(100), true);
        book.record("a", Duration::from_millis(600), true);
        book.record("a", Duration::from_millis(0), false);
        let h = book.get("a").unwrap();
        assert_eq!(h.samples, 3);
        assert!((h.latency_ms - 200.0).abs() < 1e-6, "{}", h.latency_ms);
        assert!((h.failure_rate - 0.2).abs() < 1e-6);
    }
}
