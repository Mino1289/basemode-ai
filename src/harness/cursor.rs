//! Cursor.
//!
//! Cursor reads `~/.cursor/hooks.json` (or `.cursor/hooks.json` per project) and,
//! when "Include Third-Party Plugins, Skills, and Other Configs" is on — the
//! default — it ALSO reads `~/.claude/settings.json` and maps Claude Code's hook
//! events onto its own. That compatibility is why base already half-works in
//! Cursor, and why this module has to be careful not to wire a second copy of
//! the same hook into a machine where both paths are live.
//!
//! Event mapping (Cursor's name ← Claude's):
//!
//! | Cursor | Claude Code |
//! |---|---|
//! | `sessionStart` | `SessionStart` |
//! | `beforeSubmitPrompt` | `UserPromptSubmit` |
//! | `preToolUse` | `PreToolUse` |
//! | `postToolUse` | `PostToolUse` |
//! | `stop` | `Stop` |
//! | `sessionEnd` | `SessionEnd` |
//!
//! Cursor's hook file is the operator's: this machine already has rtk and
//! ponytail entries in it. Wiring appends and never rewrites an entry it did not
//! write; un-wiring removes only entries whose command names `base hook`.
//!
//! Payload: Cursor follows Claude's shape (`tool_name`, `tool_input`,
//! `session_id`, `cwd`) with its own tool names (`Shell` for `Bash`, `Write` for
//! `Edit`, `Read` for `Read`, `Grep` for `Grep`). It has no `Glob`, so a Glob
//! matcher must never be wired here.

use std::path::{Path, PathBuf};

use super::{Binding, Harness, WireOutcome};

/// Cursor's payload in the engine's shape.
///
/// TODO(adapter): map Cursor's tool names onto the ones the engine's matchers
/// use, and leave every field it does not touch alone.
pub fn inbound(_event: &str, raw: serde_json::Value) -> serde_json::Value {
    raw
}

/// Cursor reads Claude Code's `hookSpecificOutput.additionalContext`, so the
/// engine's envelope needs no translation.
pub fn outbound(_event: &str, text: &str) -> Option<String> {
    Some(text.to_string())
}

pub struct CursorBinding;

pub static BINDING: CursorBinding = CursorBinding;

impl Binding for CursorBinding {
    fn harness(&self) -> Harness {
        Harness::Cursor
    }

    fn detected(&self, home: &Path) -> bool {
        home.join(".cursor").is_dir()
    }

    fn config_path(&self, home: &Path) -> PathBuf {
        home.join(".cursor").join("hooks.json")
    }

    fn wire(&self, _home: &Path, _exe: &Path) -> anyhow::Result<WireOutcome> {
        anyhow::bail!("the Cursor adapter is not implemented in this build")
    }

    fn unwire(&self, _home: &Path) -> anyhow::Result<WireOutcome> {
        anyhow::bail!("the Cursor adapter is not implemented in this build")
    }
}
