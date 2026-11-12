//! End to end against a real haunt core. Copy into haunt's `cli/tests/` and run
//! `cargo test -p haunt-cli --features plugins --test focus_block` (see README).
#![cfg(feature = "plugins")]

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use haunt_core::plugin_store::{PluginSet, Store, attach};
use haunt_core::plugins::{HostRequest, PluginHost};
use haunt_core::{Core, SurfaceEnv};

const ID: &str = "dev.haunt.focus-block";

fn repo() -> PathBuf {
    std::env::var_os("HAUNT_PLUGINS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../haunt-plugins"))
        .join("haunt-focus-block")
}

fn send(sock: &mut UnixStream, env: &SurfaceEnv, nonce: bool, rest: &str) {
    let nonce = if nonce { format!(r#","nonce":"{}""#, env.nonce) } else { String::new() };
    let line = format!(r#"{{"v":1,"agent":"claude","surface_id":"{}"{nonce},"session_id":"s1","cwd":"/tmp/api",{rest}}}"#, env.surface_id);
    sock.write_all(format!("{line}\n").as_bytes()).unwrap();
}

fn hold_requests(set: &Mutex<PluginSet>) -> Vec<String> {
    set.lock().unwrap().take_requests().into_iter().filter_map(|r| match r {
        HostRequest::HoldNotifications { plugin_id, minutes, .. } if plugin_id == ID => Some(format!("hold {minutes}")),
        HostRequest::HoldSummary { plugin_id, headline, actions } if plugin_id == ID => {
            Some(format!("summary {} {:?}", headline.unwrap_or_default(), actions.iter().map(|a| (a.id.as_str(), a.title.as_str())).collect::<Vec<_>>()))
        }
        HostRequest::ReleaseNotifications { plugin_id } if plugin_id == ID => Some("release".into()),
        _ => None,
    }).collect()
}

fn until(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(5), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_focus_block_holds_pings_and_counts_down_on_top() {
    let dir = repo();
    if !dir.join("haunt-plugin.toml").is_file() {
        eprintln!("skipped: haunt-focus-block not found; set HAUNT_PLUGINS_DIR");
        return;
    }
    let home = PathBuf::from(format!("/tmp/haunt-fp-focus-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&dir)
        .env_remove("CARGO_TARGET_DIR")
        .status()
        .unwrap();
    assert!(status.success());
    let o = Command::new(env!("CARGO_BIN_EXE_haunt")).args(["plugin", "install", dir.to_str().unwrap(), "--yes"]).env("HAUNT_HOME", &home).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));

    let core = Core::new(&home);
    core.start().unwrap();
    let (env, other) = (core.register_surface("/tmp").unwrap(), core.register_surface("/tmp").unwrap());
    let set = Store::open(&home).unwrap().load_all(&PluginHost::new().unwrap(), Some(&core.handle())).unwrap();
    assert!(set.errors().is_empty(), "{:?}", set.errors());
    let set = Arc::new(Mutex::new(set));
    attach(&core, &set);
    let top = || core.snapshot().top;

    let titles: Vec<_> = set.lock().unwrap().palette_items(None, "focus").into_iter().map(|i| (i.id, i.title)).collect();
    assert_eq!(titles, [("start".to_owned(), "Start a 25-minute focus block".to_owned())]);
    set.lock().unwrap().invoke(ID, "start", true).unwrap();
    assert_eq!(hold_requests(&set), ["hold 25"]);
    until("the top row", || top().is_some());
    let t = top().unwrap();
    assert_eq!((t.title.as_str(), t.sub.as_deref(), t.clickable), ("focus", Some("0 held · prompts still break through"), false));
    assert!(t.right.as_deref().is_some_and(|r| r.ends_with(" left")), "{:?}", t.right);

    let mut sock = UnixStream::connect(&env.socket_path).unwrap();
    send(&mut sock, &env, true, r#""event":"prompt_submit","query":"go""#);
    send(&mut sock, &other, false, r#""event":"stop""#);
    send(&mut sock, &env, true, r#""event":"stop""#);
    until("the held count", || top().and_then(|t| t.sub).as_deref() == Some("1 held · prompts still break through"));

    let titles: Vec<_> = set.lock().unwrap().palette_items(None, "focus").into_iter().map(|i| i.id).collect();
    assert_eq!(titles, ["end"], "only End while a block runs");
    set.lock().unwrap().invoke(ID, "end", true).unwrap();
    assert_eq!(hold_requests(&set), [r#"summary Block done · 1 thing waited [("go", "Go through them"), ("more", "5 more min")]"#, "release"], "the mockup's headline and buttons on Haunt's summary");
    assert_eq!(set.lock().unwrap().hold_surface(ID, "go"), Some(env.surface_id.clone()), "Go through them brings up the held tab");
    assert_eq!(set.lock().unwrap().hold_surface(ID, "more"), None);
    until("the top row to go", || top().is_none());
    let titles: Vec<_> = set.lock().unwrap().palette_items(None, "focus").into_iter().map(|i| i.id).collect();
    assert_eq!(titles, ["start"]);

    set.lock().unwrap().notify_action(ID, "more", "hold").unwrap();
    assert_eq!(hold_requests(&set), ["hold 5"], "5 more min from the summary");
    until("the short block's top row", || top().is_some());

    send(&mut sock, &env, true, r#""event":"permission_seen","tool_name":"Bash","tool_input":{"command":"git push"}"#);
    until("the held tab", || top().and_then(|t| t.sub).as_deref() == Some("1 held · prompts still break through"));
    core.unregister_surface(&env.surface_id);
    set.lock().unwrap().invoke(ID, "end", true).unwrap();
    assert_eq!(hold_requests(&set), [r#"summary Block done · 1 thing waited [("more", "5 more min")]"#, "release"], "a closed tab drops only its button");

    drop(core);
    let _ = std::fs::remove_dir_all(&home);
}
