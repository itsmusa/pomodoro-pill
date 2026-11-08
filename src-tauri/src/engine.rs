//! Pure timer state machine. See docs/DESIGN.md.
//!
//! This module has no Tauri, I/O, or clock access: every method that needs the
//! current time takes it as an `Instant` parameter.

use std::time::{Duration, Instant};

const WARN_THRESHOLD: Duration = Duration::from_secs(60);
const SUSPEND_GAP: Duration = Duration::from_secs(2);
const NANOS_PER_SEC: u128 = 1_000_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Focus,
    Break,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ready,
    Running,
    Paused,
}

#[derive(Clone, PartialEq, Eq, Debug, serde::Serialize)]
pub struct Snapshot {
    pub phase: Phase,
    pub status: Status,
    pub remaining_secs: u32,
    pub total_secs: u32,
    pub warn: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Event {
    Changed,
    PhaseEnded(Phase),
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub focus: Duration,
    pub brk: Duration,
    pub idle_reset: Duration,
}

pub struct Engine {
    cfg: Config,
    phase: Phase,
    status: Status,
    remaining: Duration,
    deadline: Option<Instant>,
    idle_since: Option<Instant>,
    last_tick: Option<Instant>,
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            phase: Phase::Focus,
            status: Status::Ready,
            remaining: cfg.focus,
            deadline: None,
            idle_since: None,
            last_tick: None,
        }
    }

    #[allow(dead_code)] // wired up by settings in M6
    pub fn set_config(&mut self, cfg: Config) {
        self.cfg = cfg;
        if self.status == Status::Ready {
            self.remaining = self.phase_len(self.phase);
            self.deadline = None;
        }
    }

    pub fn start(&mut self, now: Instant) {
        if self.status != Status::Ready {
            return;
        }
        let len = self.phase_len(self.phase);
        self.remaining = len;
        self.deadline = Some(now + len);
        self.status = Status::Running;
        self.idle_since = None;
        self.last_tick = Some(now);
    }

    pub fn pause(&mut self, now: Instant) {
        if self.status != Status::Running {
            return;
        }
        self.remaining = self.remaining_at(now);
        self.deadline = None;
        self.status = Status::Paused;
        self.idle_since = Some(now);
    }

    pub fn resume(&mut self, now: Instant) {
        if self.status != Status::Paused {
            return;
        }
        self.deadline = Some(now + self.remaining);
        self.status = Status::Running;
        self.idle_since = None;
        self.last_tick = Some(now);
    }

    pub fn toggle(&mut self, now: Instant) {
        match self.status {
            Status::Ready => self.start(now),
            Status::Running => self.pause(now),
            Status::Paused => self.resume(now),
        }
    }

    pub fn stop(&mut self) {
        self.phase = Phase::Focus;
        self.status = Status::Ready;
        self.remaining = self.cfg.focus;
        self.deadline = None;
        self.idle_since = None;
        self.last_tick = None;
    }

    pub fn skip(&mut self) {
        if self.phase == Phase::Break {
            self.stop();
        }
    }

    pub fn suspend(&mut self, now: Instant) {
        self.pause(now);
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Event> {
        if self.status == Status::Running {
            if let Some(last_tick) = self.last_tick {
                if now.saturating_duration_since(last_tick) > SUSPEND_GAP {
                    self.suspend(now);
                    return vec![Event::Changed];
                }
            }

            if let Some(deadline) = self.deadline {
                if now >= deadline {
                    let finished = self.phase;
                    self.phase = match finished {
                        Phase::Focus => Phase::Break,
                        Phase::Break => Phase::Focus,
                    };
                    self.status = Status::Ready;
                    self.remaining = self.phase_len(self.phase);
                    self.deadline = None;
                    self.idle_since = if self.phase == Phase::Break {
                        Some(now)
                    } else {
                        None
                    };
                    self.last_tick = Some(now);
                    return vec![Event::PhaseEnded(finished)];
                }
            }

            self.last_tick = Some(now);
        }

        if let Some(idle_since) = self.idle_since {
            if now.saturating_duration_since(idle_since) >= self.cfg.idle_reset {
                self.stop();
                return vec![Event::Changed];
            }
        }

        Vec::new()
    }

    pub fn snapshot(&self, now: Instant) -> Snapshot {
        let remaining = self.remaining_at(now);
        let total = self.phase_len(self.phase);
        Snapshot {
            phase: self.phase,
            status: self.status,
            remaining_secs: ceil_secs(remaining),
            total_secs: ceil_secs(total),
            warn: self.phase == Phase::Focus
                && self.status == Status::Running
                && remaining <= WARN_THRESHOLD,
        }
    }

    fn phase_len(&self, phase: Phase) -> Duration {
        match phase {
            Phase::Focus => self.cfg.focus,
            Phase::Break => self.cfg.brk,
        }
    }

    fn remaining_at(&self, now: Instant) -> Duration {
        if self.status == Status::Running {
            if let Some(deadline) = self.deadline {
                return deadline.saturating_duration_since(now);
            }
        }
        self.remaining
    }
}

fn ceil_secs(duration: Duration) -> u32 {
    let secs = duration.as_nanos().div_ceil(NANOS_PER_SEC);
    secs.min(u32::MAX as u128) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Instant {
        Instant::now()
    }

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    fn ms(base: Instant, millis: u64) -> Instant {
        base + Duration::from_millis(millis)
    }

    fn make_engine(focus_secs: u64, break_secs: u64) -> Engine {
        Engine::new(Config {
            focus: Duration::from_secs(focus_secs),
            brk: Duration::from_secs(break_secs),
            idle_reset: Duration::from_secs(15 * 60),
        })
    }

    /// Drives a fresh engine from FocusReady to BreakReady with 1 s ticks.
    fn enter_break_ready(engine: &mut Engine, t0: Instant, focus_secs: u64) {
        engine.start(t0);
        for sec in 1..=focus_secs {
            engine.tick(at(t0, sec));
        }
    }

    fn assert_focus_ready(engine: &Engine, now: Instant, focus_secs: u32) {
        let snap = engine.snapshot(now);
        assert_eq!(snap.phase, Phase::Focus);
        assert_eq!(snap.status, Status::Ready);
        assert_eq!(snap.remaining_secs, focus_secs);
        assert_eq!(snap.total_secs, focus_secs);
    }

    #[test]
    fn pause_and_resume_keep_remaining_exact() {
        let t0 = base();
        let mut engine = make_engine(60, 300);

        engine.start(t0);
        engine.pause(at(t0, 10));
        engine.resume(at(t0, 15));

        assert_eq!(engine.snapshot(at(t0, 15)).remaining_secs, 50);
        assert_eq!(engine.snapshot(at(t0, 20)).remaining_secs, 45);
    }

    #[test]
    fn tick_at_deadline_ends_focus_once() {
        let t0 = base();
        let mut engine = make_engine(10, 30);
        engine.start(t0);

        for sec in 1..10 {
            assert!(engine.tick(at(t0, sec)).is_empty());
        }

        assert_eq!(
            engine.tick(at(t0, 10)),
            vec![Event::PhaseEnded(Phase::Focus)]
        );
        let snap = engine.snapshot(at(t0, 10));
        assert_eq!(snap.phase, Phase::Break);
        assert_eq!(snap.status, Status::Ready);
        assert_eq!(snap.remaining_secs, 30);
        assert!(engine.tick(at(t0, 10)).is_empty());
    }

    #[test]
    fn late_tick_ends_focus_once() {
        let t0 = base();
        let mut engine = make_engine(10, 30);
        engine.start(t0);

        for sec in 1..=9 {
            assert!(engine.tick(at(t0, sec)).is_empty());
        }

        // 1 s late; the gap from the previous tick is exactly 2 s.
        assert_eq!(
            engine.tick(at(t0, 11)),
            vec![Event::PhaseEnded(Phase::Focus)]
        );
        assert_eq!(engine.snapshot(at(t0, 11)).status, Status::Ready);
        assert_eq!(engine.snapshot(at(t0, 11)).phase, Phase::Break);
        assert!(engine.tick(at(t0, 11)).is_empty());
    }

    #[test]
    fn stop_from_non_ready_states_returns_focus_ready() {
        let t0 = base();

        let mut engine = make_engine(10, 30);
        engine.start(t0);
        engine.stop();
        assert_focus_ready(&engine, at(t0, 1), 10);

        let mut engine = make_engine(10, 30);
        engine.start(t0);
        engine.pause(at(t0, 5));
        engine.stop();
        assert_focus_ready(&engine, at(t0, 6), 10);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.stop();
        assert_focus_ready(&engine, at(t0, 10), 10);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        engine.stop();
        assert_focus_ready(&engine, at(t0, 11), 10);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        engine.pause(at(t0, 11));
        engine.stop();
        assert_focus_ready(&engine, at(t0, 12), 10);
    }

    #[test]
    fn skip_from_break_returns_focus_ready() {
        let t0 = base();

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.skip();
        assert_focus_ready(&engine, at(t0, 10), 10);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        engine.skip();
        assert_focus_ready(&engine, at(t0, 11), 10);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        engine.pause(at(t0, 11));
        engine.skip();
        assert_focus_ready(&engine, at(t0, 12), 10);

        // In Focus, skip is a no-op in every status.
        let mut engine = make_engine(10, 30);
        engine.skip();
        assert_eq!(engine.snapshot(t0).status, Status::Ready);

        engine.start(t0);
        engine.skip();
        assert_eq!(engine.snapshot(at(t0, 1)).status, Status::Running);

        engine.pause(at(t0, 2));
        engine.skip();
        assert_eq!(engine.snapshot(at(t0, 2)).status, Status::Paused);
    }

    #[test]
    fn suspend_pauses_running_states_only() {
        let t0 = base();

        let mut engine = make_engine(60, 30);
        engine.start(t0);
        engine.suspend(at(t0, 5));
        assert_eq!(engine.snapshot(at(t0, 5)).status, Status::Paused);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        engine.suspend(at(t0, 11));
        assert_eq!(engine.snapshot(at(t0, 11)).status, Status::Paused);

        // No-ops: FocusReady, FocusPaused, BreakReady, BreakPaused.
        let mut engine = make_engine(60, 30);
        engine.suspend(t0);
        assert_eq!(engine.snapshot(t0).status, Status::Ready);

        engine.start(t0);
        engine.pause(at(t0, 1));
        let before = engine.snapshot(at(t0, 2));
        engine.suspend(at(t0, 2));
        assert_eq!(engine.snapshot(at(t0, 2)), before);

        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.suspend(at(t0, 10));
        assert_eq!(engine.snapshot(at(t0, 10)).status, Status::Ready);

        engine.start(at(t0, 10));
        engine.pause(at(t0, 11));
        let before = engine.snapshot(at(t0, 12));
        engine.suspend(at(t0, 12));
        assert_eq!(engine.snapshot(at(t0, 12)), before);
    }

    #[test]
    fn tick_gap_over_two_seconds_pauses() {
        let t0 = base();
        let mut engine = make_engine(60, 30);
        engine.start(t0);

        assert_eq!(engine.tick(ms(t0, 2500)), vec![Event::Changed]);
        let snap = engine.snapshot(ms(t0, 2500));
        assert_eq!(snap.status, Status::Paused);
        assert_eq!(snap.remaining_secs, 58);
    }

    #[test]
    fn idle_reset_fires_after_fifteen_minutes() {
        let t0 = base();
        let idle = Duration::from_secs(15 * 60);

        // Paused
        let mut engine = make_engine(60, 30);
        engine.start(t0);
        engine.pause(at(t0, 10));
        assert!(engine
            .tick(at(t0, 10) + idle - Duration::from_millis(1))
            .is_empty());
        assert_eq!(engine.tick(at(t0, 10) + idle), vec![Event::Changed]);
        assert_focus_ready(&engine, at(t0, 10) + idle, 60);

        // BreakReady
        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        assert_eq!(engine.tick(at(t0, 10) + idle), vec![Event::Changed]);
        assert_focus_ready(&engine, at(t0, 10) + idle, 10);

        // BreakPaused
        let mut engine = make_engine(10, 30);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        engine.pause(at(t0, 11));
        assert_eq!(engine.tick(at(t0, 11) + idle), vec![Event::Changed]);
        assert_focus_ready(&engine, at(t0, 11) + idle, 10);

        // FocusReady has no idle timer.
        let mut engine = make_engine(60, 30);
        assert!(engine.tick(t0 + idle).is_empty());
        assert_eq!(engine.snapshot(t0 + idle).status, Status::Ready);

        // Running has no idle timer, even after 15 minutes of ticks.
        let mut engine = make_engine(1000, 30);
        engine.start(t0);
        for sec in 1..=900 {
            assert!(engine.tick(at(t0, sec)).is_empty());
        }
        assert_eq!(engine.snapshot(at(t0, 900)).status, Status::Running);
    }

    #[test]
    fn warn_threshold_and_conditions() {
        let t0 = base();

        let mut engine = make_engine(120, 120);
        engine.start(t0);
        assert!(!engine.snapshot(at(t0, 59)).warn); // 61 s left
        assert!(engine.snapshot(at(t0, 60)).warn); // 60 s left

        engine.pause(at(t0, 70)); // 50 s left, paused
        assert!(!engine.snapshot(at(t0, 70)).warn);

        // Break never warns.
        let mut engine = make_engine(10, 120);
        enter_break_ready(&mut engine, t0, 10);
        engine.start(at(t0, 10));
        let snap = engine.snapshot(at(t0, 70));
        assert_eq!(snap.phase, Phase::Break);
        assert_eq!(snap.status, Status::Running);
        assert_eq!(snap.remaining_secs, 60);
        assert!(!snap.warn);
    }

    #[test]
    fn remaining_secs_rounds_up() {
        let t0 = base();
        let mut engine = make_engine(60, 30);
        engine.start(t0);

        assert_eq!(engine.snapshot(ms(t0, 900)).remaining_secs, 60); // 59.1 s
        assert_eq!(engine.snapshot(ms(t0, 59_800)).remaining_secs, 1); // 0.2 s
    }

    #[test]
    fn toggle_walks_states() {
        let t0 = base();
        let mut engine = make_engine(60, 30);

        assert_eq!(engine.snapshot(t0).status, Status::Ready);
        engine.toggle(t0);
        assert_eq!(engine.snapshot(t0).status, Status::Running);
        engine.toggle(at(t0, 1));
        assert_eq!(engine.snapshot(at(t0, 1)).status, Status::Paused);
        engine.toggle(at(t0, 2));
        assert_eq!(engine.snapshot(at(t0, 2)).status, Status::Running);
    }

    #[test]
    fn invalid_calls_change_nothing() {
        let t0 = base();
        let mut engine = make_engine(60, 30);
        engine.start(t0);

        let before = engine.snapshot(at(t0, 10));
        engine.resume(at(t0, 10));
        assert_eq!(engine.snapshot(at(t0, 10)), before);

        engine.pause(at(t0, 10));
        let before = engine.snapshot(at(t0, 20));
        engine.start(at(t0, 20));
        assert_eq!(engine.snapshot(at(t0, 20)), before);
    }
}
