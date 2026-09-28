//! Display recovery policy, independent of the physical I2C driver.

#[derive(Default)]
pub struct Recovery {
    failures: u8,
    retry_at_ms: u64,
}

impl Recovery {
    pub fn ready(&self, now_ms: u64) -> bool {
        now_ms >= self.retry_at_ms
    }

    pub fn failed(&mut self, now_ms: u64) -> u64 {
        let delay_ms = (1_000u64 << self.failures.min(5)).min(30_000);
        self.failures = self.failures.saturating_add(1);
        self.retry_at_ms = now_ms.saturating_add(delay_ms);
        delay_ms
    }

    pub fn recovered(&mut self) {
        *self = Self::default();
    }

    pub fn wait_ms(&self, now_ms: u64) -> u64 {
        self.retry_at_ms.saturating_sub(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_back_off_and_cap_without_blocking_new_frames() {
        let mut recovery = Recovery::default();
        let mut now = 0;
        for expected in [1000, 2000, 4000, 8000, 16000, 30000, 30000] {
            assert!(recovery.ready(now));
            assert_eq!(recovery.failed(now), expected);
            assert!(!recovery.ready(now + expected - 1));
            now += expected;
        }
        recovery.recovered();
        assert!(recovery.ready(now));
        assert_eq!(recovery.failed(now), 1000);
    }

    #[test]
    fn deadlines_saturate_and_wait_never_underflows() {
        let mut recovery = Recovery::default();
        recovery.failed(u64::MAX - 1);
        assert_eq!(recovery.wait_ms(u64::MAX), 0);
        assert!(recovery.ready(u64::MAX));
    }
}
