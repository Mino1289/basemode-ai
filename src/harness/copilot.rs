//! GitHub Copilot CLI.
//!
//! User-level hooks live in `~/.copilot/hooks/*.json` (honouring `COPILOT_HOME`
//! when set) and repo-level ones in `.github/hooks/*.json`, which must be
//! committed. base writes one file of its own, `base.json`, so un-wiring is a
//! delete rather than a merge: no other tool's hooks share that file.
//!
//! ```json
//! { "version": 1, "hooks": {
//!   "sessionStart": [{ "type": "command",
//!     "bash": "base hook session-start --harness copilot",
//!     "powershell": "base hook session-start --harness copilot",
//!     "timeoutSec": 10 }] } }
//! ```
//!
//! Events: `sessionStart`, `sessionEnd`, `userPromptSubmitted`, `preToolUse`,
//! `postToolUse`, `errorOccurred`, `agentStop`. Injection is a single line,
//! `{"additionalContext": "..."}`, on stdout.
//!
//! Known upstream bugs to design around (all open as of 2026-10):
//!
//! - `sessionStart` `additionalContext` is ignored on some versions (#2142).
//! - `sessionStart` fires per prompt rather than once per session (#2491).
//! - `preToolUse` ignores `updatedInput` (#2013).
//!
//! So: Copilot is injection-only. Nothing may depend on a `sessionStart` hook
//! having run, the engine must tolerate `session-start` arriving many times,
//! and every hook fails open.

use std::path::{Path, PathBuf};

use super::{Binding, Harness, WireOutcome};

/// Copilot's payload in the engine's shape.
///
/// TODO(adapter): map Copilot's tool names and payload fields onto the ones the
/// engine's matchers read, and leave every field it does not touch alone.
pub fn inbound(_event: &str, raw: serde_json::Value) -> serde_json::Value {
    raw
}

/// One line of `{"additionalContext": …}`, which is all Copilot reads.
pub fn outbound(_event: &str, text: &str) -> Option<String> {
    Some(serde_json::json!({ "additionalContext": super::context_of(text) }).to_string())
}

pub struct CopilotBinding;

pub static BINDING: CopilotBinding = CopilotBinding;

impl Binding for CopilotBinding {
    fn harness(&self) -> Harness {
        Harness::Copilot
    }

    fn detected(&self, home: &Path) -> bool {
        copilot_home(home).is_dir()
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        copilot_home(home).join("hooks").join("base.json")
    }

    fn wire(&self, _home: &Path, _exe: &Path) -> anyhow::Result<WireOutcome> {
        anyhow::bail!("the Copilot adapter is not implemented in this build")
    }

    fn unwire(&self, _home: &Path) -> anyhow::Result<WireOutcome> {
        anyhow::bail!("the Copilot adapter is not implemented in this build")
    }
}

/// `COPILOT_HOME` when set, else `~/.copilot`.
fn copilot_home(home: &Path) -> PathBuf {
    match std::env::var_os("COPILOT_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".copilot"),
    }
}
