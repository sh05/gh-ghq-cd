use std::cell::Cell;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

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

/// Extracts a non-empty string from a herdr CLI JSON response at the given
/// JSON Pointer (herdr wraps every payload as `{"id":...,"result":{...}}`).
fn herdr_response_str(output: &str, pointer: &str) -> Result<String> {
    let value: serde_json::Value =
        serde_json::from_str(output).context("failed to parse herdr output as JSON")?;
    let field = value
        .pointer(pointer)
        .and_then(|v| v.as_str())
        .with_context(|| format!("herdr output has no string value at {pointer}"))?;
    if field.is_empty() {
        bail!("herdr output has an empty value at {pointer}");
    }
    Ok(field.to_string())
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
        // The workspace_created response carries only workspace metadata (no
        // pane_id), so look up the new workspace's initial pane separately.
        let workspace_id = herdr_response_str(&output, "/result/workspace/workspace_id")
            .context("herdr workspace create did not return a workspace_id")?;
        let output = runner.run("herdr", &["pane", "list", "--workspace", &workspace_id])?;
        let initial_pane_id = herdr_response_str(&output, "/result/panes/0/pane_id")
            .context("herdr pane list did not return the new workspace's pane")?;

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
            let second_pane_id = herdr_response_str(&output, "/result/pane/pane_id")
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
        let pane_a = herdr_response_str(&output, "/result/pane/pane_id")
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
            let pane_b = herdr_response_str(&output, "/result/pane/pane_id")
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
    fn herdr_response_str_extracts_pane_id_from_split_response() {
        let json = r#"{"id":"cli:pane:split","result":{"type":"pane_created","pane":{"cwd":"/tmp","focused":false,"pane_id":"w8:p2","tab_id":"w8:t1","workspace_id":"w8"}}}"#;
        assert_eq!(
            herdr_response_str(json, "/result/pane/pane_id").unwrap(),
            "w8:p2"
        );
    }

    #[test]
    fn herdr_response_str_extracts_from_array() {
        let json = r#"{"id":"cli:pane:list","result":{"panes":[{"pane_id":"w8:p1","workspace_id":"w8"}],"type":"pane_list"}}"#;
        assert_eq!(
            herdr_response_str(json, "/result/panes/0/pane_id").unwrap(),
            "w8:p1"
        );
    }

    #[test]
    fn herdr_response_str_missing_field_errors() {
        let json = r#"{"id":"cli:workspace:create","result":{"type":"workspace_created","workspace":{"workspace_id":"w9"}}}"#;
        assert!(herdr_response_str(json, "/result/pane/pane_id").is_err());
    }

    #[test]
    fn herdr_response_str_empty_value_errors() {
        let json = r#"{"result":{"pane":{"pane_id":""}}}"#;
        assert!(herdr_response_str(json, "/result/pane/pane_id").is_err());
    }

    #[test]
    fn herdr_response_str_invalid_json_errors() {
        assert!(herdr_response_str("not json", "/result/pane/pane_id").is_err());
    }

    #[test]
    fn herdr_response_str_handles_escaped_quotes_in_other_fields() {
        let json = r#"{"result":{"pane":{"cwd":"/tmp/say \"hi\"","label":"pane_id","pane_id":"w8:p3"}}}"#;
        assert_eq!(
            herdr_response_str(json, "/result/pane/pane_id").unwrap(),
            "w8:p3"
        );
    }
}
