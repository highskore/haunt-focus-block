//! Focus block: start a 25-minute block from the ⌘K palette. Haunt holds agent notifications (never
//! a permission prompt), a row at the top of the sidebar counts down, and the block ends with one
//! summary of what piled up, which Haunt posts.
// wit-bindgen's flattened `notify` import has 14 params.
#![allow(clippy::too_many_arguments)]

wit_bindgen::generate!({ path: "wit/haunt-plugin.wit", world: "plugin" });

// Shared with the other fleet-following plugins; not every helper is used here.
#[allow(dead_code)]
mod fleet;
mod focus;

use std::sync::Mutex;

use fleet::{Ev, Fleet};
use focus::{Block, Held};
use haunt::plugin::log::log;
use haunt::plugin::rows::Tone;
use haunt::plugin::top_row::{self, Top};
use haunt::plugin::types::LogLevel;
use haunt::plugin::hold::SummaryAction;
use haunt::plugin::{hold, palette, timer};
use serde::Deserialize;

const TICK_SECS: u32 = 15;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub minutes: i64,
    /// What "5 more min" adds.
    pub extend_minutes: i64,
    pub unverified: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { minutes: 25, extend_minutes: 5, unverified: false }
    }
}

impl Config {
    fn focus(&self) -> focus::Settings {
        focus::Settings { minutes: self.minutes.clamp(1, 240) as u64, extend_minutes: self.extend_minutes.clamp(1, 60) as u64 }
    }
}

#[derive(Default)]
struct Live {
    config: Config,
    fleet: Fleet,
    block: Option<Block>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Live) -> R) -> R {
    f(LIVE.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Live::default))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// The sidebar's top row: `focus`, `17:42 left`, `3 held · prompts still break through`, a bar.
pub fn top(b: &Block, now: u64) -> Top {
    Top { title: "focus".into(), sub: Some(b.meta()), right: Some(b.left(now)), tone: Tone::Text, bar: Some(b.fraction(now)), action: None }
}

/// Only the command that applies is listed.
fn commands(l: &Live) {
    if l.block.is_some() {
        palette::remove("start");
        palette::command("end", "End focus block", None);
    } else {
        palette::remove("end");
        palette::command("start", &format!("Start a {}-minute focus block", l.config.focus().minutes), None);
    }
}

fn draw(l: &Live, now: u64) {
    let row = l.block.as_ref().map(|b| top(b, now));
    if let Err(e) = top_row::set(row.as_ref()) {
        log(LogLevel::Warn, &format!("focus-block: no top row: {e}"));
    }
}

fn start(l: &mut Live, minutes: u64, now: u64) {
    if let Err(e) = hold::start(minutes as u32) {
        log(LogLevel::Warn, &format!("focus-block: notifications not held: {e}"));
        return;
    }
    let mut b = Block::start(now, &l.config.focus());
    b.ends = now + minutes * focus::MIN_MS;
    l.block = Some(b);
    let _ = timer::every(TICK_SECS);
}

/// Ends the block: Haunt lets notifications through again and posts one summary, headed by ours.
fn end(l: &mut Live) {
    let Some(b) = l.block.take() else { return };
    let more = SummaryAction { id: "more".into(), label: format!("{} more min", l.config.focus().extend_minutes.clamp(1, 60)), surface: None };
    let go = b.first_to_see().map(|s| SummaryAction { id: "go".into(), label: "Go through them".into(), surface: Some(s.to_owned()) });
    let mut res = hold::set_summary(Some(&b.headline()), &go.into_iter().chain([more.clone()]).collect::<Vec<_>>());
    if res.is_err() {
        // That tab closed during the block; the summary still comes, without the button.
        res = hold::set_summary(Some(&b.headline()), &[more]);
    }
    if let Err(e) = res {
        log(LogLevel::Warn, &format!("focus-block: no headline: {e}"));
    }
    hold::release();
    timer::stop();
}

struct Plugin;

impl Guest for Plugin {
    fn init(json: String) {
        with(|l| {
            l.config = serde_json::from_str(&json).unwrap_or_default();
            commands(l);
        });
    }

    fn on_settings_changed(json: String) {
        with(|l| {
            l.config = serde_json::from_str(&json).unwrap_or_default();
            commands(l);
        });
    }

    fn on_event(event: AgentEvent) {
        let Some(surface) = event.surface.as_deref() else { return };
        with(|l| {
            if !event.verified && !l.config.unverified {
                return;
            }
            let now = now_ms();
            let ev = Ev { surface, kind: &event.kind, cwd: event.cwd.as_deref(), summary: event.summary.as_deref(), subagent: event.agent_id.is_some() };
            let Some(change) = l.fleet.apply(&ev, now) else { return };
            if let Some(b) = l.block.as_mut()
                && b.hold(Held { surface: surface.to_owned(), what: change })
            {
                draw(l, now);
            }
        });
    }

    fn palette_items(_mode: String, _query: String) -> Vec<PaletteItem> {
        Vec::new()
    }

    fn on_action(action_id: String, _payload: String) {
        with(|l| {
            let now = now_ms();
            match action_id.as_str() {
                "start" if l.block.is_none() => start(l, l.config.focus().minutes, now),
                "more" if l.block.is_none() => start(l, l.config.focus().extend_minutes, now),
                "end" => end(l),
                _ => return,
            }
            commands(l);
            draw(l, now);
        });
    }

    fn row_menu(_surface: String) -> Vec<MenuItem> {
        Vec::new()
    }

    fn on_row_moved(_surface: String, _group: String, _before: Option<String>) {}

    fn on_theme_changed() {}

    fn on_tick(now_ms: u64) {
        with(|l| {
            if l.block.as_ref().is_some_and(|b| b.over(now_ms)) {
                end(l);
                commands(l);
            }
            draw(l, now_ms);
        });
    }

    fn on_exec(_job: u64, _outcome: Result<haunt::plugin::exec::Output, String>) {}

    fn on_away_changed(_away: bool, _away_ms: u64) {}

    fn on_hover_hold(_held: bool, _surface: String) {}

    fn on_fs_tree(_job: u64, _outcome: Result<haunt::plugin::fs_list::TreeResult, String>) {}
}

export!(Plugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_top_row_reads_like_the_mockup() {
        let b = Block::start(0, &focus::Settings::default());
        let t = top(&b, 25 * focus::MIN_MS - (17 * 60 + 42) * 1000);
        assert_eq!((t.title.as_str(), t.right.as_deref(), t.sub.as_deref()), ("focus", Some("17:42 left"), Some("0 held · prompts still break through")));
        assert!((t.bar.unwrap() - 0.29).abs() < 0.01);
    }

    #[test]
    fn manifest_asks_for_what_it_uses_and_defaults_match() {
        let m = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/haunt-plugin.toml")).unwrap();
        let t: toml::Table = toml::from_str(&m).unwrap();
        let caps = t["capabilities"].as_table().unwrap();
        assert_eq!(caps.keys().collect::<Vec<_>>(), ["events", "notify_hold", "sidebar_top", "timer"]);
        let defaults: serde_json::Map<String, serde_json::Value> =
            t["settings"].as_table().unwrap().iter().map(|(k, v)| (k.clone(), serde_json::to_value(&v["default"]).unwrap())).collect();
        let c = serde_json::from_value::<Config>(serde_json::Value::Object(defaults)).unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.focus(), focus::Settings::default());
    }
}
