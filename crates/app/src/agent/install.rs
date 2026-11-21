//! Settings → AI agents → Install: adds pdfr to the agents found on this
//! computer, so the user does not have to edit their settings by hand.
//!
//! For each agent this registers the `pdfr mcp` server in the agent's own
//! settings and, where the agent reads skills, writes a short guide on using
//! pdfr (`SKILL.md`) next to its other skills. Nothing else in the agent's
//! settings is touched, and a settings file that cannot be read is left alone.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The guide written for agents that read skills.
pub const SKILL: &str = include_str!("SKILL.md");

/// The name pdfr is registered under, and its skill's folder.
const NAME: &str = "pdfr";

/// An agent pdfr can add itself to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Client {
    ClaudeCode,
    Codex,
    Cursor,
    ClaudeDesktop,
}

/// The command an agent runs to reach pdfr: the program and its arguments
/// (ending in `mcp`).
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub cmd: String,
    pub args: Vec<String>,
}

impl Client {
    pub const ALL: [Client; 4] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::Cursor,
        Client::ClaudeDesktop,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::ClaudeDesktop => "Claude Desktop",
        }
    }

    /// The agent's own folder under the home folder: there if it is
    /// installed.
    fn dir(self, home: &Path) -> PathBuf {
        match self {
            Client::ClaudeCode => home.join(".claude"),
            Client::Codex => home.join(".codex"),
            Client::Cursor => home.join(".cursor"),
            Client::ClaudeDesktop if cfg!(target_os = "macos") => {
                home.join("Library/Application Support/Claude")
            }
            Client::ClaudeDesktop => home.join(".config/Claude"),
        }
    }

    /// The settings file that lists the agent's MCP servers.
    fn config(self, home: &Path) -> PathBuf {
        match self {
            Client::ClaudeCode => home.join(".claude.json"),
            Client::Codex => self.dir(home).join("config.toml"),
            Client::Cursor => self.dir(home).join("mcp.json"),
            Client::ClaudeDesktop => self.dir(home).join("claude_desktop_config.json"),
        }
    }

    /// Where the guide goes, for the agents that read skills.
    pub fn skill_path(self, home: &Path) -> Option<PathBuf> {
        match self {
            Client::ClaudeCode | Client::Codex => {
                Some(self.dir(home).join("skills").join(NAME).join("SKILL.md"))
            }
            Client::Cursor | Client::ClaudeDesktop => None,
        }
    }

    /// Whether this agent is on the computer.
    pub fn found(self, home: &Path) -> bool {
        self.dir(home).is_dir()
    }

    /// Whether pdfr is already in this agent's settings.
    pub fn installed(self, home: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(self.config(home)) else {
            return false;
        };
        match self {
            Client::Codex => toml_has_server(&text),
            _ => serde_json::from_str::<Value>(&text)
                .is_ok_and(|v| v["mcpServers"].get(NAME).is_some()),
        }
    }

    /// Adds pdfr to this agent. May take a moment (Claude Code is asked
    /// through its own command), so call it off the UI thread.
    pub fn install(self, home: &Path, launch: &Launch) -> Result<(), String> {
        if !self.installed(home) {
            match self {
                Client::ClaudeCode => claude_code_add(home, launch)?,
                Client::Codex => toml_add(&self.config(home), launch)?,
                Client::Cursor | Client::ClaudeDesktop => json_add(&self.config(home), launch)?,
            }
        }
        if let Some(path) = self.skill_path(home) {
            write_skill(&path)?;
        }
        Ok(())
    }
}

/// The user's home folder.
pub fn home() -> Option<PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn said(what: &str, path: &Path, e: impl std::fmt::Display) -> String {
    format!("Could not {what} {}: {e}.", path.display())
}

/// Writes the guide, replacing an older copy of it.
fn write_skill(path: &Path) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| said("create", dir, e))?;
    }
    std::fs::write(path, SKILL).map_err(|e| said("write", path, e))
}

/// Replaces a settings file in one step, so a crash half way leaves the old
/// one. A symlinked file (dotfiles) stays a symlink: its target is replaced.
fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).map_err(|e| said("create", dir, e))?;
    }
    let mut tmp = target.clone().into_os_string();
    tmp.push(".pdfr-new");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text).map_err(|e| said("write", &tmp, e))?;
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        said("write", &target, e)
    })
}

/// Adds pdfr under `mcpServers` in a JSON settings file (Cursor, Claude
/// Desktop), keeping everything else in it.
fn json_add(path: &Path, launch: &Launch) -> Result<(), String> {
    let mut root = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => {
            serde_json::from_str::<Value>(&text).map_err(|e| said("read", path, e))?
        }
        Ok(_) => json!({}),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(e) => return Err(said("read", path, e)),
    };
    let not_settings = || said("read", path, "it is not a settings file pdfr understands");
    let servers = root
        .as_object_mut()
        .ok_or_else(not_settings)?
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(not_settings)?;
    servers.insert(
        NAME.into(),
        json!({"command": launch.cmd, "args": launch.args}),
    );
    let mut text = serde_json::to_string_pretty(&root).map_err(|e| said("write", path, e))?;
    text.push('\n');
    replace_file(path, &text)
}

/// Whether a Codex `config.toml` already has pdfr's table.
fn toml_has_server(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        l == "[mcp_servers.pdfr]" || l == "[mcp_servers.\"pdfr\"]"
    })
}

/// Adds pdfr's table at the end of a Codex `config.toml`. A new table at the
/// end is valid whatever comes before it, so the file need not be parsed.
fn toml_add(path: &Path, launch: &Launch) -> Result<(), String> {
    let mut text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(said("read", path, e)),
    };
    if toml_has_server(&text) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    // A JSON string is a valid TOML basic string, and so is a JSON list of
    // strings a TOML array.
    text.push_str(&format!(
        "[mcp_servers.{NAME}]\ncommand = {}\nargs = {}\n",
        json!(launch.cmd),
        json!(launch.args),
    ));
    replace_file(path, &text)
}

/// The `claude` command: on the PATH, or where its installers put it (a
/// desktop app's PATH is often shorter than a terminal's).
fn claude_cli(home: &Path) -> Option<PathBuf> {
    if let Some(p) = pdfr_assist::ai::which("claude") {
        return Some(p);
    }
    [
        home.join(".local/bin/claude"),
        home.join(".claude/local/claude"),
        home.join(".local/share/mise/command-wrappers/bin/claude"),
        home.join(".bun/bin/claude"),
        home.join(".npm-global/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        PathBuf::from("/opt/homebrew/bin/claude"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// Claude Code keeps its servers in a file it rewrites while it runs, so it
/// is asked to add pdfr itself: `claude mcp add --scope user pdfr -- …`.
fn claude_code_add(home: &Path, launch: &Launch) -> Result<(), String> {
    let cli = claude_cli(home).ok_or(
        "Could not find the claude command. Copy the command below and run it in a terminal.",
    )?;
    let out = std::process::Command::new(&cli)
        .args(["mcp", "add", "--scope", "user", NAME, "--"])
        .arg(&launch.cmd)
        .args(&launch.args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| said("run", &cli, e))?;
    if out.status.success() {
        return Ok(());
    }
    let text = [out.stderr, out.stdout]
        .iter()
        .map(|b| String::from_utf8_lossy(b).trim().to_string())
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| "it failed".into());
    Err(format!("Claude Code said: {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty home folder of its own for a test.
    fn home_for(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfr-install-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn launch() -> Launch {
        Launch {
            cmd: "/opt/My Apps/pdfr".into(),
            args: vec!["mcp".into()],
        }
    }

    #[test]
    fn only_agents_on_the_computer_are_found() {
        let home = home_for("found");
        assert!(Client::ALL.iter().all(|c| !c.found(&home)));
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        assert!(Client::Cursor.found(&home));
        assert!(!Client::Codex.found(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn cursor_keeps_its_other_servers() {
        let home = home_for("cursor");
        let dir = home.join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mcp.json"),
            r#"{"mcpServers": {"other": {"command": "x"}}, "theme": "dark"}"#,
        )
        .unwrap();
        assert!(!Client::Cursor.installed(&home));
        Client::Cursor.install(&home, &launch()).unwrap();
        assert!(Client::Cursor.installed(&home));
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["mcpServers"]["pdfr"]["command"], "/opt/My Apps/pdfr");
        assert_eq!(v["mcpServers"]["pdfr"]["args"], json!(["mcp"]));
        // No guide: Cursor is told how by the server itself.
        assert_eq!(Client::Cursor.skill_path(&home), None);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_settings_file_that_cannot_be_read_is_left_alone() {
        let home = home_for("broken");
        let dir = home.join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mcp.json"), "{ not json").unwrap();
        assert!(Client::Cursor.install(&home, &launch()).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("mcp.json")).unwrap(),
            "{ not json"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn claude_desktop_gets_a_settings_file_if_it_has_none() {
        let home = home_for("desktop");
        let dir = Client::ClaudeDesktop.dir(&home);
        std::fs::create_dir_all(&dir).unwrap();
        Client::ClaudeDesktop.install(&home, &launch()).unwrap();
        assert!(Client::ClaudeDesktop.installed(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn codex_gets_a_table_and_the_guide() {
        let home = home_for("codex");
        let dir = home.join(".codex");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("config.toml"),
            "model = \"x\"\n\n[mcp_servers.other]\ncommand = \"y\"",
        )
        .unwrap();
        assert!(!Client::Codex.installed(&home));
        Client::Codex.install(&home, &launch()).unwrap();
        // A second time changes nothing.
        Client::Codex.install(&home, &launch()).unwrap();
        let text = std::fs::read_to_string(dir.join("config.toml")).unwrap();
        assert_eq!(
            text,
            "model = \"x\"\n\n[mcp_servers.other]\ncommand = \"y\"\n\n\
             [mcp_servers.pdfr]\ncommand = \"/opt/My Apps/pdfr\"\nargs = [\"mcp\"]\n"
        );
        assert!(Client::Codex.installed(&home));
        let skill = std::fs::read_to_string(dir.join("skills/pdfr/SKILL.md")).unwrap();
        assert_eq!(skill, SKILL);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_guide_names_only_tools_that_exist() {
        assert!(SKILL.starts_with("---\nname: pdfr\ndescription:"));
        let tools: Vec<String> = super::super::mcp::tools()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        // Every `word` in backticks that looks like a tool is one.
        for word in SKILL.split('`').skip(1).step_by(2) {
            if word.contains('_') && word.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
                let known = tools.iter().any(|t| t == word)
                    || [
                        "only_empty",
                        "double_click",
                        "paused_by_user",
                        "needs_confirmation",
                        "no_window",
                    ]
                    .contains(&word);
                assert!(known, "the guide mentions `{word}`");
            }
        }
        for t in ["open", "read_form", "fill", "check_form", "save_as"] {
            assert!(
                SKILL.contains(&format!("`{t}`")),
                "the guide leaves out {t}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_settings_file_stays_a_symlink() {
        let home = home_for("symlink");
        let dir = home.join(".cursor");
        std::fs::create_dir_all(&dir).unwrap();
        let real = home.join("dotfiles-mcp.json");
        std::fs::write(&real, "{}").unwrap();
        std::os::unix::fs::symlink(&real, dir.join("mcp.json")).unwrap();
        Client::Cursor.install(&home, &launch()).unwrap();
        assert!(dir.join("mcp.json").is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("pdfr"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
