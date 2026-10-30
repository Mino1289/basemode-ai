//! OpenCode 2.x.
//!
//! OpenCode has no hook config file: it loads TypeScript plugins from
//! `~/.config/opencode/plugins/` (global) or `.opencode/plugins/` (project).
//! So "wiring" here is writing one file, `base.ts`, that shells out to
//! `base hook <event> --harness opencode` and applies the JSON answer.
//!
//! What the plugin can do, and what base uses it for:
//!
//! | OpenCode | base event | How the answer lands |
//! |---|---|---|
//! | `ctx.session.hook("prompt")` | `user-prompt-submit` | append to `event.prompt.text` |
//! | `ctx.session.hook("context")` | `pre-tool-use`, `post-tool-use` | `event.system.push(...)` |
//! | `ctx.event.subscribe("session.created")` | `session-start` | `event.system.push(...)` |
//! | `ctx.event.subscribe("session.idle")` | `stop` | side effects only |
//! | `ctx.event.subscribe("session.deleted")` | `session-end` | side effects only |
//!
//! Constraints the plugin must respect (learned from the rtk plugin on this
//! machine):
//!
//! - `Plugin.define({ id, setup })` — the 2.x loader schema-validates a plain
//!   object; a callable default export is rejected.
//! - `child_process.execFile`, not Bun's `$`: OpenCode runs under Electron,
//!   which has no `$`.
//! - `BASE_HEADLESS=1` short-circuits the plugin, or base's own
//!   `opencode run` extraction calls re-enter the hooks.
//! - Every failure is swallowed: a plugin that throws costs the user their
//!   turn, and a hook is an optimization.

use std::path::{Path, PathBuf};

use super::{Binding, Harness, WireOutcome};

/// The plugin speaks the canonical event names and sends the engine's own
/// payload shape, so there is nothing to normalize.
pub fn inbound(raw: serde_json::Value) -> serde_json::Value {
    raw
}

/// The plugin reads one field, `additionalContext`, from the answer.
pub fn outbound(_event: &str, text: &str) -> Option<String> {
    Some(serde_json::json!({ "additionalContext": super::context_of(text) }).to_string())
}

pub struct OpenCodeBinding;

pub static BINDING: OpenCodeBinding = OpenCodeBinding;

impl Binding for OpenCodeBinding {
    fn harness(&self) -> Harness {
        Harness::OpenCode
    }

    fn detected(&self, home: &Path) -> bool {
        home.join(".config").join("opencode").is_dir()
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        home.join(".config").join("opencode").join("plugins").join("base.ts")
    }

    fn wire(&self, _home: &Path, _exe: &Path) -> anyhow::Result<WireOutcome> {
        anyhow::bail!("the OpenCode adapter is not implemented in this build")
    }

    fn unwire(&self, _home: &Path) -> anyhow::Result<WireOutcome> {
        anyhow::bail!("the OpenCode adapter is not implemented in this build")
    }
}
