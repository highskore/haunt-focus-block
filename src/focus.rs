//! A focus block: its clock, the pings it holds, and the summary it ends with. Free of host calls.

use crate::fleet::{Change, State};

pub const MIN_MS: u64 = 60_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub minutes: u64,
    pub extend_minutes: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self { minutes: 25, extend_minutes: 5 }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    pub surface: String,
    pub what: Change,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Block {
    pub started: u64,
    pub ends: u64,
    pub held: Vec<Held>,
}

impl Block {
    pub fn start(now: u64, s: &Settings) -> Self {
        Self { started: now, ends: now + s.minutes.clamp(1, 240) * MIN_MS, held: Vec::new() }
    }

    pub fn extend(&mut self, now: u64, s: &Settings) {
        self.ends = self.ends.max(now) + s.extend_minutes.clamp(1, 60) * MIN_MS;
    }

    pub fn over(&self, now: u64) -> bool {
        now >= self.ends
    }

    /// Elapsed share of the block, 0 to 1.
    pub fn fraction(&self, now: u64) -> f32 {
        let total = self.ends.saturating_sub(self.started).max(1);
        (now.saturating_sub(self.started).min(total) as f32) / total as f32
    }

    /// `17:42 left`.
    pub fn left(&self, now: u64) -> String {
        let s = self.ends.saturating_sub(now).div_ceil(1000);
        format!("{}:{:02} left", s / 60, s % 60)
    }

    /// Whether a ping for this change goes through now; if not it is held, the latest per tab.
    /// Keeps the tab's latest change; starting work isn't news. Haunt never holds a permission
    /// prompt's notification, but it still counts here.
    pub fn hold(&mut self, h: Held) -> bool {
        if h.what.to == State::Running {
            return false;
        }
        self.held.retain(|x| x.surface != h.surface);
        self.held.push(h);
        true
    }

    /// `3 held · prompts still break through`.
    pub fn meta(&self) -> String {
        format!("{} held · prompts still break through", self.held.len())
    }

    /// The tab "Go through them" brings up: the longest still waiting on you, else the first held.
    pub fn first_to_see(&self) -> Option<&str> {
        self.held.iter().find(|h| h.what.to == State::NeedsYou).or(self.held.first()).map(|h| h.surface.as_str())
    }

    /// The headline of Haunt's summary, which lists what waited under it.
    pub fn headline(&self) -> String {
        match self.held.len() {
            0 => "Block done · nothing waited".to_owned(),
            1 => "Block done · 1 thing waited".to_owned(),
            n => format!("Block done · {n} things waited"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(surface: &str, to: State) -> Held {
        Held { surface: surface.into(), what: Change { from: Some(State::Running), to } }
    }

    #[test]
    fn go_through_them_starts_with_who_waits_on_you() {
        let mut b = Block::start(0, &Settings::default());
        assert_eq!(b.first_to_see(), None);
        b.hold(held("d", State::Done));
        assert_eq!(b.first_to_see(), Some("d"));
        b.hold(held("a", State::NeedsYou));
        b.hold(held("w", State::NeedsYou));
        assert_eq!(b.first_to_see(), Some("a"));
    }

    #[test]
    fn the_clock_reads_like_the_mockup() {
        let s = Settings::default();
        let b = Block::start(0, &s);
        let now = 25 * MIN_MS - (17 * 60 + 42) * 1000;
        assert_eq!(b.left(now), "17:42 left");
        assert!((b.fraction(now) - 0.29).abs() < 0.01);
        assert!(!b.over(now) && b.over(25 * MIN_MS));
    }

    #[test]
    fn piles_up_the_latest_change_per_tab() {
        let mut b = Block::start(0, &Settings::default());
        assert!(b.hold(held("a", State::NeedsYou)));
        assert!(b.hold(held("d", State::Done)));
        assert!(b.hold(held("w", State::NeedsYou)));
        assert!(!b.hold(held("x", State::Running)));
        assert_eq!(b.meta(), "3 held · prompts still break through");
        assert_eq!(b.headline(), "Block done · 3 things waited");
    }

    #[test]
    fn one_entry_per_tab_the_latest() {
        let s = Settings::default();
        let mut b = Block::start(0, &s);
        b.hold(held("a", State::NeedsYou));
        b.hold(held("a", State::Done));
        assert_eq!(b.held.len(), 1);
        assert_eq!(b.held[0].what.to, State::Done);
        assert_eq!(Block::start(0, &s).headline(), "Block done · nothing waited");
    }

    #[test]
    fn five_more_minutes() {
        let s = Settings::default();
        let mut b = Block::start(0, &s);
        b.extend(30 * MIN_MS, &s);
        assert_eq!(b.ends, 35 * MIN_MS, "extends from now once over");
    }
}
