use std::cell::Cell;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::command::{CommandRunner, SystemCommandRunner};
use crate::environment::Environment;

pub struct WindowConfig {
    pub name: String,
    pub start_dir: PathBuf,
}

impl WindowConfig {
    pub fn new<S: Into<String>, P: Into<PathBuf>>(name: S, start_dir: P) -> Self {
        Self {
            name: name.into(),
            start_dir: start_dir.into(),
        }
    }
}

pub trait Multiplexer {
    fn new_window(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()>;
    fn rename_window(&self, name: &str) -> Result<()>;
    fn new_pane(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()>;
    fn send_keys(&self, keys: &str) -> Result<()>;
}

pub struct TmuxClient;
pub struct ZellijClient;

pub struct HerdrClient {
    pane_id: Option<String>,
    workspace_id: Option<String>,
    last_target_pane: Cell<Option<String>>,
}

impl HerdrClient {
    pub fn new(env: &dyn Environment) -> Self {
        Self {
            pane_id: env.var("HERDR_PANE_ID"),
            workspace_id: env.var("HERDR_WORKSPACE_ID"),
            last_target_pane: Cell::new(None),
        }
    }
}

pub struct NoopClient;

/// Extracts a string field's value from a JSON response by scanning for
/// `"field":"value"` (with an optional space after the colon), without
/// pulling in a full JSON parser dependency.
fn extract_json_field(json: &str, field: &str) -> Option<String> {
    let key_pattern = format!("\"{field}\"");
    let key_start = json.find(&key_pattern)?;
    let after_key = &json[key_start + key_pattern.len()..];
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_colon = after_colon.trim_start();
    let value_start = after_colon.strip_prefix('"')?;
    let value_end = value_start.find('"')?;
    Some(value_start[..value_end].to_string())
}

impl Multiplexer for TmuxClient {
    fn new_window(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()> {
        let runner = SystemCommandRunner;
        let start_dir = cfg
            .start_dir
            .to_str()
            .context("repository path contains invalid UTF-8")?;

        runner.run("tmux", &["new-window", "-n", &cfg.name, "-c", start_dir])?;

        // If pane_count >= 2, split the new window into 2 panes
        // (the new window itself is the "lane", so we only need to split it)
        if pane_count >= 2 {
            // Split direction:
            // - vertical (default): -v (split top/bottom)
            // - horizontal: -h (split left/right)
            let split = if horizontal { "-h" } else { "-v" };
            runner.run("tmux", &["split-window", split, "-c", start_dir])?;

            // Navigate and set titles for both panes
            let nav_to_first = if horizontal { "-L" } else { "-U" };
            let nav_to_second = if horizontal { "-R" } else { "-D" };

            runner.run("tmux", &["select-pane", nav_to_first])?;
            runner.run("tmux", &["select-pane", "-T", &cfg.name])?;

            runner.run("tmux", &["select-pane", nav_to_second])?;
            runner.run("tmux", &["select-pane", "-T", &cfg.name])?;

            // Return to first pane (focus)
            runner.run("tmux", &["select-pane", nav_to_first])?;

            // Equalize pane sizes
            runner.run("tmux", &["select-layout", "-E"])?;
        }

        Ok(())
    }

    fn rename_window(&self, name: &str) -> Result<()> {
        let runner = SystemCommandRunner;
        runner.run("tmux", &["rename-window", name])?;
        Ok(())
    }

    fn new_pane(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()> {
        let runner = SystemCommandRunner;
        let start_dir = cfg
            .start_dir
            .to_str()
            .context("repository path contains invalid UTF-8")?;

        // Primary split direction:
        // - vertical (default): -hf (horizontal split with full height, creates left/right)
        // - horizontal: -vf (vertical split with full width, creates top/bottom)
        let primary_split = if horizontal { "-vf" } else { "-hf" };
        runner.run("tmux", &["split-window", primary_split, "-c", start_dir])?;

        // Set pane title for the new pane
        runner.run("tmux", &["select-pane", "-T", &cfg.name])?;

        if pane_count >= 2 {
            // Secondary split (perpendicular to primary):
            // - vertical primary: -v (split top/bottom within the new pane)
            // - horizontal primary: -h (split left/right within the new pane)
            let secondary_split = if horizontal { "-h" } else { "-v" };
            runner.run("tmux", &["split-window", secondary_split, "-c", start_dir])?;

            // Navigate and set titles for both sub-panes
            let nav_to_first = if horizontal { "-L" } else { "-U" };
            let nav_to_second = if horizontal { "-R" } else { "-D" };

            runner.run("tmux", &["select-pane", nav_to_first])?;
            runner.run("tmux", &["select-pane", "-T", &cfg.name])?;

            runner.run("tmux", &["select-pane", nav_to_second])?;
            runner.run("tmux", &["select-pane", "-T", &cfg.name])?;

            // Return to first sub-pane (focus)
            runner.run("tmux", &["select-pane", nav_to_first])?;
        }

        // Equalize pane sizes
        runner.run("tmux", &["select-layout", "-E"])?;

        Ok(())
    }

    fn send_keys(&self, keys: &str) -> Result<()> {
        let runner = SystemCommandRunner;
        runner.run("tmux", &["send-keys", keys, "Enter"])?;
        Ok(())
    }
}

impl Multiplexer for ZellijClient {
    fn new_window(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()> {
        let runner = SystemCommandRunner;
        let start_dir = cfg
            .start_dir
            .to_str()
            .context("repository path contains invalid UTF-8")?;

        runner.run(
            "zellij",
            &["action", "new-tab", "--name", &cfg.name, "--cwd", start_dir],
        )?;

        // Set pane name for the initial pane
        runner.run("zellij", &["action", "rename-pane", &cfg.name])?;

        // If pane_count >= 2, split the new tab into 2 panes
        if pane_count >= 2 {
            // Split direction:
            // - vertical (default): down (split top/bottom)
            // - horizontal: right (split left/right)
            let direction = if horizontal { "right" } else { "down" };
            runner.run(
                "zellij",
                &[
                    "action",
                    "new-pane",
                    "--direction",
                    direction,
                    "--cwd",
                    start_dir,
                ],
            )?;

            // Set pane name for the new pane
            runner.run("zellij", &["action", "rename-pane", &cfg.name])?;

            // Move focus back to first pane
            let focus_direction = if horizontal { "left" } else { "up" };
            runner.run("zellij", &["action", "move-focus", focus_direction])?;
        }

        Ok(())
    }

    fn rename_window(&self, name: &str) -> Result<()> {
        let runner = SystemCommandRunner;
        runner.run("zellij", &["action", "rename-tab", name])?;
        Ok(())
    }

    fn new_pane(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()> {
        let runner = SystemCommandRunner;
        let start_dir = cfg
            .start_dir
            .to_str()
            .context("repository path contains invalid UTF-8")?;

        // Primary split direction:
        // - vertical (default): right (split left/right)
        // - horizontal: down (split top/bottom)
        let primary_direction = if horizontal { "down" } else { "right" };
        runner.run(
            "zellij",
            &[
                "action",
                "new-pane",
                "--direction",
                primary_direction,
                "--cwd",
                start_dir,
            ],
        )?;

        // Set pane name for the new pane
        runner.run("zellij", &["action", "rename-pane", &cfg.name])?;

        if pane_count >= 2 {
            // Secondary split (perpendicular to primary):
            let secondary_direction = if horizontal { "right" } else { "down" };
            runner.run(
                "zellij",
                &[
                    "action",
                    "new-pane",
                    "--direction",
                    secondary_direction,
                    "--cwd",
                    start_dir,
                ],
            )?;

            // Set pane name for the second pane
            runner.run("zellij", &["action", "rename-pane", &cfg.name])?;

            // Move focus back to first sub-pane
            let focus_direction = if horizontal { "left" } else { "up" };
            runner.run("zellij", &["action", "move-focus", focus_direction])?;
        }

        Ok(())
    }

    fn send_keys(&self, keys: &str) -> Result<()> {
        let runner = SystemCommandRunner;
        // Write the command characters
        runner.run("zellij", &["action", "write-chars", keys])?;
        // Send Enter key (newline = 10 in ASCII)
        runner.run("zellij", &["action", "write", "10"])?;
        Ok(())
    }
}

impl Multiplexer for HerdrClient {
    fn new_window(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()> {
        let runner = SystemCommandRunner;
        let start_dir = cfg
            .start_dir
            .to_str()
            .context("repository path contains invalid UTF-8")?;

        // Herdr's own convention is one workspace per repo/task/investigation
        // (tabs are for different views *within* the same project), so a new
        // "window" for a different repository maps to a new workspace.
        let output = runner.run(
            "herdr",
            &[
                "workspace",
                "create",
                "--cwd",
                start_dir,
                "--label",
                &cfg.name,
                "--focus",
            ],
        )?;
        let initial_pane_id = extract_json_field(&output, "pane_id")
            .context("herdr workspace create did not return a pane_id")?;

        runner.run("herdr", &["pane", "rename", &initial_pane_id, &cfg.name])?;

        // Regardless of pane_count, send_keys (if called next) should target
        // this initial pane.
        self.last_target_pane.set(Some(initial_pane_id.clone()));

        if pane_count >= 2 {
            // Split direction:
            // - vertical (default): down (split top/bottom)
            // - horizontal: right (split left/right)
            let direction = if horizontal { "right" } else { "down" };
            // Keep focus on the initial pane, mirroring tmux/zellij's
            // "return focus to first pane" end state.
            let output = runner.run(
                "herdr",
                &[
                    "pane",
                    "split",
                    &initial_pane_id,
                    "--direction",
                    direction,
                    "--cwd",
                    start_dir,
                    "--no-focus",
                ],
            )?;
            let second_pane_id = extract_json_field(&output, "pane_id")
                .context("herdr pane split did not return a pane_id")?;
            runner.run("herdr", &["pane", "rename", &second_pane_id, &cfg.name])?;
        }

        Ok(())
    }

    fn rename_window(&self, name: &str) -> Result<()> {
        let runner = SystemCommandRunner;
        // A "window" maps to a Herdr workspace (see new_window), so renaming
        // it renames the current workspace, not the current tab.
        let workspace_id = self
            .workspace_id
            .as_deref()
            .context("HERDR_WORKSPACE_ID is not set; are you running inside a Herdr pane?")?;
        runner.run("herdr", &["workspace", "rename", workspace_id, name])?;
        Ok(())
    }

    fn new_pane(&self, cfg: &WindowConfig, pane_count: u8, horizontal: bool) -> Result<()> {
        let runner = SystemCommandRunner;
        let start_dir = cfg
            .start_dir
            .to_str()
            .context("repository path contains invalid UTF-8")?;
        let source_pane_id = self
            .pane_id
            .as_deref()
            .context("HERDR_PANE_ID is not set; are you running inside a Herdr pane?")?;

        // Primary split direction (inverted vs new_window, matching the
        // existing Tmux/Zellij clients):
        // - vertical (default): right (split left/right)
        // - horizontal: down (split top/bottom)
        let primary_direction = if horizontal { "down" } else { "right" };
        let output = runner.run(
            "herdr",
            &[
                "pane",
                "split",
                source_pane_id,
                "--direction",
                primary_direction,
                "--cwd",
                start_dir,
                "--focus",
            ],
        )?;
        let pane_a = extract_json_field(&output, "pane_id")
            .context("herdr pane split did not return a pane_id")?;
        runner.run("herdr", &["pane", "rename", &pane_a, &cfg.name])?;

        self.last_target_pane.set(Some(pane_a.clone()));

        if pane_count >= 2 {
            // Secondary split (perpendicular to primary):
            let secondary_direction = if horizontal { "right" } else { "down" };
            let output = runner.run(
                "herdr",
                &[
                    "pane",
                    "split",
                    &pane_a,
                    "--direction",
                    secondary_direction,
                    "--cwd",
                    start_dir,
                    "--no-focus",
                ],
            )?;
            let pane_b = extract_json_field(&output, "pane_id")
                .context("herdr pane split did not return a pane_id")?;
            runner.run("herdr", &["pane", "rename", &pane_b, &cfg.name])?;
        }

        Ok(())
    }

    fn send_keys(&self, keys: &str) -> Result<()> {
        let runner = SystemCommandRunner;
        let target = self
            .last_target_pane
            .take()
            .or_else(|| self.pane_id.clone())
            .context("no target pane available to send keys to")?;
        runner.run("herdr", &["pane", "run", &target, keys])?;
        Ok(())
    }
}

impl Multiplexer for NoopClient {
    fn new_window(&self, _: &WindowConfig, _: u8, _: bool) -> Result<()> {
        Ok(())
    }
    fn rename_window(&self, _: &str) -> Result<()> {
        Ok(())
    }
    fn new_pane(&self, _: &WindowConfig, _: u8, _: bool) -> Result<()> {
        Ok(())
    }
    fn send_keys(&self, _: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_field_no_space_after_colon() {
        let json = r#"{"pane_id":"pane-123","tab_id":"tab-456"}"#;
        assert_eq!(
            extract_json_field(json, "pane_id"),
            Some("pane-123".to_string())
        );
    }

    #[test]
    fn extract_json_field_space_after_colon() {
        let json = r#"{"pane_id": "pane-123", "tab_id": "tab-456"}"#;
        assert_eq!(
            extract_json_field(json, "pane_id"),
            Some("pane-123".to_string())
        );
    }

    #[test]
    fn extract_json_field_missing_field_returns_none() {
        let json = r#"{"tab_id":"tab-456"}"#;
        assert_eq!(extract_json_field(json, "pane_id"), None);
    }

    #[test]
    fn extract_json_field_empty_value() {
        let json = r#"{"pane_id":""}"#;
        assert_eq!(extract_json_field(json, "pane_id"), Some(String::new()));
    }

    #[test]
    fn extract_json_field_with_trailing_content() {
        let json = r#"{"tab_id":"tab-456","pane_id":"pane-123","workspace_id":"ws-1"}"#;
        assert_eq!(
            extract_json_field(json, "pane_id"),
            Some("pane-123".to_string())
        );
    }
}
