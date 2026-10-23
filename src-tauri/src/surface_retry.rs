//! One delayed wake-up per canvas after a transient surface acquisition failure.
//! A successful/hidden/failed render invalidates older callbacks.
#[derive(Default)]
pub(crate) struct SurfaceRetry {
    generation: u64,
    queued: Option<u64>,
    attempts: u32,
}

impl SurfaceRetry {
    pub(crate) fn schedule(&mut self) -> Option<(u64, u64)> {
        if self.queued.is_some() {
            return None;
        }
        self.generation = self.generation.wrapping_add(1);
        self.queued = Some(self.generation);
        let delay_ms = (16u64 << self.attempts.min(6)).min(1000);
        self.attempts = self.attempts.saturating_add(1);
        Some((self.generation, delay_ms))
    }

    pub(crate) fn wake(&mut self, generation: u64) -> bool {
        if self.queued != Some(generation) {
            return false;
        }
        self.queued = None;
        true
    }

    pub(crate) fn cancel(&mut self) {
        self.queued = None;
        self.attempts = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_retry_coalesces_and_backs_off_without_losing_wakeups() {
        let mut state = SurfaceRetry::default();
        for expected in [16, 32, 64, 128, 256, 512, 1000, 1000] {
            let (generation, delay) = state.schedule().unwrap();
            assert_eq!(delay, expected);
            assert!(state.schedule().is_none());
            assert!(!state.wake(generation.wrapping_add(1)));
            assert!(state.wake(generation));
            assert!(!state.wake(generation));
        }
        // Presentation, hiding, or a hard error cancels the outstanding request.
        let (old, _) = state.schedule().unwrap();
        state.cancel();
        assert!(!state.wake(old));
        let (fresh, delay) = state.schedule().unwrap();
        assert_eq!(delay, 16);
        assert_ne!(old, fresh);
        assert!(!state.wake(old));
        assert!(state.wake(fresh));
    }
}
