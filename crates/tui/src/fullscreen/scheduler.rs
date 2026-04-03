use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderIntent {
    None,
    Schedule,
    Render,
}

#[derive(Debug, Clone)]
pub struct RenderScheduler {
    frame_interval: Duration,
    idle_interval: Duration,
    dirty: bool,
    frame_deadline: Option<Instant>,
    idle_deadline: Option<Instant>,
}

impl Default for RenderScheduler {
    fn default() -> Self {
        Self::new(Duration::from_millis(33), Duration::from_millis(12))
    }
}

impl RenderScheduler {
    pub fn new(frame_interval: Duration, idle_interval: Duration) -> Self {
        Self {
            frame_interval,
            idle_interval,
            dirty: false,
            frame_deadline: None,
            idle_deadline: None,
        }
    }

    pub fn mark_dirty(&mut self, now: Instant) {
        if !self.dirty {
            self.frame_deadline = Some(now + self.frame_interval);
        }
        self.dirty = true;
        self.idle_deadline = Some(now + self.idle_interval);
    }

    pub fn next_flush_at(&self) -> Option<Instant> {
        match (self.frame_deadline, self.idle_deadline) {
            (Some(frame), Some(idle)) => Some(frame.min(idle)),
            (Some(frame), None) => Some(frame),
            (None, Some(idle)) => Some(idle),
            (None, None) => None,
        }
    }

    pub fn should_flush(&self, now: Instant) -> bool {
        self.dirty && self.next_flush_at().is_some_and(|deadline| now >= deadline)
    }

    pub fn on_flushed(&mut self) {
        self.dirty = false;
        self.frame_deadline = None;
        self.idle_deadline = None;
    }

    pub fn clear(&mut self) {
        self.on_flushed();
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_batches_until_idle_or_frame_deadline() {
        let start = Instant::now();
        let mut scheduler =
            RenderScheduler::new(Duration::from_millis(50), Duration::from_millis(10));

        scheduler.mark_dirty(start);
        let frame_deadline = scheduler.next_flush_at().expect("deadline should exist");
        scheduler.mark_dirty(start + Duration::from_millis(5));
        scheduler.mark_dirty(start + Duration::from_millis(9));

        assert!(!scheduler.should_flush(start + Duration::from_millis(18)));
        assert!(scheduler.should_flush(start + Duration::from_millis(19)));
        assert!(frame_deadline >= start + Duration::from_millis(10));
    }

    #[test]
    fn scheduler_flushes_on_frame_cap_during_long_burst() {
        let start = Instant::now();
        let mut scheduler =
            RenderScheduler::new(Duration::from_millis(30), Duration::from_millis(10));

        scheduler.mark_dirty(start);
        scheduler.mark_dirty(start + Duration::from_millis(8));
        scheduler.mark_dirty(start + Duration::from_millis(16));
        scheduler.mark_dirty(start + Duration::from_millis(24));

        assert!(!scheduler.should_flush(start + Duration::from_millis(29)));
        assert!(scheduler.should_flush(start + Duration::from_millis(30)));
    }
}
