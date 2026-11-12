<div align="center">

<img src="docs/icon.png" width="96" alt="">

# Focus block

**A focus timer that holds agent pings until the block ends.**

[![Haunt plugin](https://img.shields.io/badge/Haunt-plugin-D97757)](https://github.com/highskore/haunt-hackathon)
![Plugin API 0.5](https://img.shields.io/badge/plugin%20API-0.5-111)
![WASM component](https://img.shields.io/badge/WASM-component-654ff0?logo=webassembly&logoColor=white)
![MIT licence](https://img.shields.io/badge/licence-MIT-6b7280)

</div>

Run **Start a 25-minute focus block** from the ⌘K palette. Haunt holds its notifications and other plugins' for the block, and a row at the top of the sidebar counts down: `focus · 17:42 left`, with `3 held · prompts still break through` and a thin bar. Haunt never holds a permission prompt: its notification still shows and its row still goes to NEEDS YOU.

When the block ends, Haunt posts one summary headed `Block done · 3 things waited`, listing the notifications that waited, with **Go through them**, which brings up the tab that has waited longest, and a **5 more min** button for a short block. **End focus block** in the palette ends it early. Other plugins may hold notifications at the same time; the summary comes once the last of them lets go.

## Settings

```toml
[plugins."dev.haunt.focus-block"]
minutes = 25         # length of a block, 1 to 240
extend_minutes = 5   # the summary button's short block, 1 to 60
unverified = false   # also follow events without the tab's nonce
```

## Capabilities

| capability | why |
|---|---|
| `events` (session start, prompt, subagent, tool, permission request, question, stop) | what each agent did during the block |
| `notify_hold` | holds notifications during the block |
| `sidebar_top` | the countdown row above the sidebar |
| `timer` | moves the countdown on and ends the block |

## Build and test

```sh
cargo build --release --target wasm32-wasip2   # or ./build.sh
cargo test                                     # unit tests
haunt plugin install .
```

`e2e/focus_block.rs` runs the built plugin against a real Haunt core: copy it into a Haunt checkout's `cli/tests/` and run `HAUNT_PLUGINS_DIR=<this repo's parent> cargo test -p haunt-cli --features plugins --test focus_block`.

Needs Haunt with plugin API 0.5 (`api = "0.5"`) and its `hold` and `top-row` imports.

## Licence

MIT, see `LICENSE`.
