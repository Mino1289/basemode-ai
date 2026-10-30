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

use serde_json::{Map, Value};

use super::{Binding, Harness, WireOutcome};

/// The six events base wires, host name first and canonical name second.
///
/// `errorOccurred` is deliberately absent: base has no arm for it, and a hook
/// that cannot do anything is a process spawn per error for nothing.
const HOOK_EVENTS: [(&str, &str); 6] = [
    ("sessionStart", "session-start"),
    ("userPromptSubmitted", "user-prompt-submit"),
    ("preToolUse", "pre-tool-use"),
    ("postToolUse", "post-tool-use"),
    ("agentStop", "stop"),
    ("sessionEnd", "session-end"),
];

/// Copilot kills a hook that outlives this. Ten seconds is the value its own
/// examples use, and base's slowest hook (a cold graph parse) stays under it.
const TIMEOUT_SECS: u64 = 10;

/// The fields a Copilot hook entry can carry its command in.
const COMMAND_KEYS: [&str; 3] = ["bash", "powershell", "command"];

/// Copilot's payload in the engine's shape.
///
/// Copilot spells three fields differently from Claude Code; everything else it
/// sends (`cwd`, `timestamp`, `prompt`, a tool's own arguments) is already where
/// the engine looks, so it is copied through untouched. The original keys stay
/// too: a hook cannot know which payload shape a future Copilot will still send,
/// and dropping one would lose information for nothing.
pub fn inbound(_event: &str, raw: serde_json::Value) -> serde_json::Value {
    let Value::Object(mut object) = raw else {
        return raw;
    };

    if let Some(session) = object.get("sessionId").cloned() {
        object.entry("session_id").or_insert(session);
    }
    if let Some(tool) = object.get("toolName").cloned() {
        object.entry("tool_name").or_insert(tool);
    }
    if let Some(args) = object.get("toolArgs").cloned() {
        object.entry("tool_input").or_insert(parse_tool_args(args));
    }

    Value::Object(object)
}

/// Copilot sends `toolArgs` as a JSON string; the engine's matchers want the
/// object itself. A string that is not JSON is passed through as a string —
/// fail open, never drop what the host sent.
fn parse_tool_args(args: Value) -> Value {
    match args {
        Value::String(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
        other => other,
    }
}

/// One line of `{"additionalContext": …}`, which is all Copilot reads.
///
/// The text is unwrapped first when the engine emitted a Claude envelope: putting
/// JSON inside `additionalContext` would show the model the envelope, not the
/// context.
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

    /// Write base's own `base.json`. Idempotent: a second call adds nothing.
    ///
    /// The file normally does not exist (it is base's alone), so this creates
    /// `hooks/` when needed. If it does exist — an older base, or an operator
    /// who put their own hooks there — entries base does not recognise are kept
    /// and only missing base events are appended.
    fn wire(&self, home: &Path, _exe: &Path) -> anyhow::Result<WireOutcome> {
        let path = self.config_path(home);
        let mut root = if path.exists() {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
            serde_json::from_str(&text)
                .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", path.display()))?
        } else {
            serde_json::json!({ "version": 1, "hooks": {} })
        };

        let object = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} is not a JSON object", path.display()))?;
        object.entry("version").or_insert(Value::from(1));
        let hooks = object
            .entry("hooks")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} has a non-object \"hooks\"", path.display()))?;

        let mut added = Vec::new();
        for (host_event, event) in HOOK_EVENTS {
            let entries = hooks
                .entry(host_event)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .ok_or_else(|| {
                    anyhow::anyhow!("{} has a non-array \"hooks.{host_event}\"", path.display())
                })?;
            if !entries.iter().any(|entry| names_event(entry, event)) {
                entries.push(hook_entry(event));
                added.push(host_event);
            }
        }

        if added.is_empty() {
            return Ok(WireOutcome::unchanged(path, "already wired"));
        }

        write_json(&path, &root)?;
        Ok(WireOutcome {
            path,
            changed: added.len(),
            note: format!("added {}", added.join(", ")),
        })
    }

    /// Remove base's entries, and the whole file when it holds nothing else.
    ///
    /// `base.json` is base's file, so a file whose every hook entry names
    /// `base hook` is deleted outright. A file that also holds another tool's
    /// entries keeps them: only base's entries go, and arrays that became empty
    /// because base's entry left are dropped.
    fn unwire(&self, home: &Path) -> anyhow::Result<WireOutcome> {
        let path = self.config_path(home);
        if !path.exists() {
            return Ok(WireOutcome::unchanged(path, "not wired"));
        }

        let text = std::fs::read_to_string(&path)
            .map_err(|e| anyhow::anyhow!("reading {}: {e}", path.display()))?;
        let mut root: Value = serde_json::from_str(&text)
            .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", path.display()))?;
        let object = root
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("{} is not a JSON object", path.display()))?;

        // Decided before anything is removed: a file is base's only when every
        // entry in it is, and it has no top-level key base never wrote.
        let only_base = object.keys().all(|key| key == "version" || key == "hooks")
            && object
                .get("hooks")
                .and_then(Value::as_object)
                .is_some_and(|hooks| {
                    !hooks.is_empty()
                        && hooks.values().all(|entries| {
                            entries
                                .as_array()
                                .is_some_and(|a| !a.is_empty() && a.iter().all(is_base_entry))
                        })
                });

        let hooks = match object.get_mut("hooks").and_then(Value::as_object_mut) {
            Some(hooks) => hooks,
            None => return Ok(WireOutcome::unchanged(path, "no base hooks")),
        };

        let mut removed = 0usize;
        for entries in hooks.values_mut() {
            if let Some(array) = entries.as_array_mut() {
                let before = array.len();
                array.retain(|entry| !is_base_entry(entry));
                removed += before - array.len();
            }
        }

        if removed == 0 {
            return Ok(WireOutcome::unchanged(path, "no base hooks"));
        }

        if only_base {
            std::fs::remove_file(&path)
                .map_err(|e| anyhow::anyhow!("deleting {}: {e}", path.display()))?;
            return Ok(WireOutcome {
                path,
                changed: removed,
                note: format!("removed {removed} entries and deleted the file"),
            });
        }

        hooks.retain(|_, entries| entries.as_array().is_none_or(|a| !a.is_empty()));
        write_json(&path, &root)?;
        Ok(WireOutcome {
            path,
            changed: removed,
            note: format!("removed {removed} entries"),
        })
    }
}

/// `COPILOT_HOME` when set, else `~/.copilot`.
fn copilot_home(home: &Path) -> PathBuf {
    match std::env::var_os("COPILOT_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".copilot"),
    }
}

fn hook_command(event: &str) -> String {
    format!("base hook {event} --harness copilot")
}

fn hook_entry(event: &str) -> Value {
    let command = hook_command(event);
    serde_json::json!({
        "type": "command",
        "bash": command.clone(),
        "powershell": command,
        "timeoutSec": TIMEOUT_SECS,
    })
}

/// Whether `entry` already serves `event`: an entry base wrote for this event,
/// with or without the `--harness copilot` suffix older releases omitted.
fn names_event(entry: &Value, event: &str) -> bool {
    let wanted = format!("base hook {event}");
    COMMAND_KEYS.iter().any(|key| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .is_some_and(|command| command.contains(&wanted))
    })
}

/// Whether every command in `entry` names base's hook. An entry with no command
/// field at all is not base's; neither is one that carries another tool's
/// command beside base's, because removing it would take that other tool's hook
/// with it.
fn is_base_entry(entry: &Value) -> bool {
    let mut found = false;
    for key in COMMAND_KEYS {
        if let Some(command) = entry.get(key).and_then(Value::as_str) {
            found = true;
            if !command.contains("base hook") {
                return false;
            }
        }
    }
    found
}

fn write_json(path: &Path, root: &Value) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| anyhow::anyhow!("creating {}: {e}", dir.display()))?;
    }
    let mut text = serde_json::to_string_pretty(root)?;
    text.push('\n');
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &text).map_err(|e| anyhow::anyhow!("writing {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| anyhow::anyhow!("renaming {} to {}: {e}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_has_a_command_for_both_shells() {
        for (host_event, event) in HOOK_EVENTS {
            let entry = hook_entry(event);
            assert_eq!(entry["type"], "command", "{host_event}");
            let expected = format!("base hook {event} --harness copilot");
            assert_eq!(entry["bash"], expected, "{host_event}");
            assert_eq!(entry["powershell"], expected, "{host_event}");
            assert_eq!(entry["timeoutSec"], TIMEOUT_SECS, "{host_event}");
        }
    }

    #[test]
    fn an_entry_with_one_command_outside_base_is_not_base_owned() {
        let mixed = serde_json::json!({
            "bash": "base hook stop --harness copilot",
            "powershell": "other-tool stop",
        });
        assert!(
            !is_base_entry(&mixed),
            "removing it would remove the other tool's hook"
        );
        assert!(is_base_entry(&hook_entry("stop")));
        assert!(!is_base_entry(&serde_json::json!({ "type": "prompt" })));
    }

    #[test]
    fn a_legacy_base_command_still_counts_as_wired() {
        let legacy = serde_json::json!({ "bash": "base hook session-start" });
        assert!(names_event(&legacy, "session-start"));
        assert!(!names_event(&legacy, "stop"));
    }

    #[test]
    fn tool_args_as_a_json_string_become_the_engine_input() {
        let got = parse_tool_args(Value::String(r#"{"command":"ls -la"}"#.into()));
        assert_eq!(got["command"], "ls -la");
    }

    #[test]
    fn a_string_that_is_not_json_survives_intact() {
        assert_eq!(parse_tool_args(Value::String("ls -la".into())), "ls -la");
        assert_eq!(parse_tool_args(Value::Null), Value::Null);
    }
}
