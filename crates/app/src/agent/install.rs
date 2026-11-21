//! Settings → AI agents → Install: adds pdfr to the agents found on this
//! computer, so the user does not have to edit their settings by hand.
//!
//! For each agent this registers the `pdfr mcp` server in the agent's own
//! settings and, where the agent reads skills, writes a short guide on using
//! pdfr (`SKILL.md`) where it looks for them. Nothing else in the agent's
//! settings is touched, and a settings file that cannot be read is left alone.
//!
//! The settings files are edited directly: an agent's own command may be a
//! launcher that installs the agent when first run. Claude Code is the one
//! exception: it rewrites its file while it runs, so it is asked itself.

use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};

/// The guide written for agents that read skills.
pub const SKILL: &str = include_str!("SKILL.md");

/// The name pdfr is registered under, and its skill's folder.
const NAME: &str = "pdfr";

/// The skills folder most agents share.
const SHARED_SKILLS: &str = ".agents/skills";

/// The command an agent runs to reach pdfr: the program and its arguments
/// (ending in `mcp`).
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub cmd: String,
    pub args: Vec<String>,
}

/// How an agent's entry for a server is written in a JSON settings file.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    /// `{"command": …, "args": […]}`
    Plain,
    /// The same with `"type": "stdio"`.
    Stdio,
    /// GitHub Copilot: `"type": "local"` and every tool allowed.
    Local,
    /// Kilo: `"type": "local"`, the command and its arguments as one list.
    List,
}

/// How pdfr is added to an agent.
#[derive(Clone, Copy, Debug, PartialEq)]
enum How {
    /// `claude mcp add --scope user …`; the servers are read from
    /// `.claude.json`.
    ClaudeCode,
    /// A JSON file with the servers under `keys`.
    Json {
        keys: &'static [&'static str],
        entry: Entry,
    },
    /// opencode: `mcp.servers.<name>` (or `mcp.<name>` in a file written
    /// for version 1), the command and its arguments as one list.
    Opencode,
    /// Codex, Grok: a `[mcp_servers.<name>]` table in `config.toml`.
    Toml,
    /// Hermes: an entry under `mcp_servers:` in `config.yaml`.
    HermesYaml,
    /// Crush: an `mcp add` line in its `crushrc`.
    Crushrc,
    /// DeepSeek Harness: an item in its YAML list of patches.
    DshYaml,
}

/// An agent pdfr can add itself to. All paths are under the home folder.
#[derive(Debug, PartialEq)]
pub struct Client {
    pub name: &'static str,
    /// The agent is on this computer if one of these folders is there.
    dirs: &'static [&'static str],
    /// The settings file that lists its MCP servers: the first one that is
    /// there, else the first whose folder is.
    files: &'static [&'static str],
    how: How,
    /// The folder it reads skills from, if it reads them.
    skills: Option<&'static str>,
}

const fn json(
    name: &'static str,
    dir: &'static [&'static str],
    files: &'static [&'static str],
    entry: Entry,
    skills: Option<&'static str>,
) -> Client {
    Client {
        name,
        dirs: dir,
        files,
        how: How::Json {
            keys: &["mcpServers"],
            entry,
        },
        skills,
    }
}

/// Every agent pdfr knows how to add itself to, as listed in Settings.
pub static CLIENTS: &[Client] = &[
    Client {
        name: "Claude Code",
        dirs: &[".claude"],
        files: &[".claude.json"],
        how: How::ClaudeCode,
        skills: Some(".claude/skills"),
    },
    json(
        "Claude Desktop",
        &[".config/Claude", "Library/Application Support/Claude"],
        &[
            ".config/Claude/claude_desktop_config.json",
            "Library/Application Support/Claude/claude_desktop_config.json",
        ],
        Entry::Plain,
        None,
    ),
    // ChatGPT Desktop and T3 Code read Codex's settings.
    Client {
        name: "Codex and ChatGPT Desktop",
        dirs: &[".codex"],
        files: &[".codex/config.toml"],
        how: How::Toml,
        skills: Some(SHARED_SKILLS),
    },
    json(
        "Cursor",
        &[".cursor"],
        &[".cursor/mcp.json"],
        Entry::Stdio,
        Some(SHARED_SKILLS),
    ),
    Client {
        name: "OpenCode",
        dirs: &[".config/opencode"],
        files: &[
            ".config/opencode/opencode.json",
            ".config/opencode/opencode.jsonc",
        ],
        how: How::Opencode,
        skills: Some(SHARED_SKILLS),
    },
    json(
        "Antigravity",
        &[".gemini/config"],
        &[".gemini/config/mcp_config.json"],
        Entry::Plain,
        Some(".gemini/config/skills"),
    ),
    json(
        "GitHub Copilot",
        &[".copilot"],
        &[".copilot/mcp-config.json"],
        Entry::Local,
        Some(SHARED_SKILLS),
    ),
    Client {
        name: "Amp",
        dirs: &[".config/amp"],
        files: &[".config/amp/settings.json", ".config/amp/settings.jsonc"],
        how: How::Json {
            keys: &["amp.mcpServers"],
            entry: Entry::Plain,
        },
        skills: Some(SHARED_SKILLS),
    },
    Client {
        name: "Crush",
        dirs: &[".config/crush"],
        files: &[".config/crush/crushrc"],
        how: How::Crushrc,
        skills: Some(SHARED_SKILLS),
    },
    json(
        "Kiro",
        &[".kiro"],
        &[".kiro/settings/mcp.json"],
        Entry::Plain,
        Some(".kiro/skills"),
    ),
    json(
        "Devin",
        &[".config/devin"],
        &[".config/devin/mcp_config.json"],
        Entry::Plain,
        Some(SHARED_SKILLS),
    ),
    json(
        "MiniMax Code",
        &[".minimax"],
        &[".minimax/mcp.json"],
        Entry::Plain,
        Some(SHARED_SKILLS),
    ),
    Client {
        name: "DeepSeek Harness",
        dirs: &[".dsh"],
        files: &[".dsh/cordis.patch.yml"],
        how: How::DshYaml,
        skills: Some(SHARED_SKILLS),
    },
    Client {
        name: "Grok",
        dirs: &[".grok"],
        files: &[".grok/config.toml"],
        how: How::Toml,
        skills: Some(SHARED_SKILLS),
    },
    // Hermes Desktop reads the same settings.
    Client {
        name: "Hermes",
        dirs: &[".hermes"],
        files: &[".hermes/config.yaml"],
        how: How::HermesYaml,
        skills: Some(".hermes/skills"),
    },
    Client {
        name: "OpenClaw",
        dirs: &[".openclaw"],
        files: &[".openclaw/openclaw.json"],
        how: How::Json {
            keys: &["mcp", "servers"],
            entry: Entry::Plain,
        },
        skills: Some(SHARED_SKILLS),
    },
    json(
        "Pi",
        &[".pi/agent"],
        &[".pi/agent/mcp.json"],
        Entry::Plain,
        Some(SHARED_SKILLS),
    ),
    json(
        "Oh My Pi",
        &[".omp/agent"],
        &[".omp/agent/mcp.json"],
        Entry::Stdio,
        None,
    ),
    json(
        "Kimi Code",
        &[".kimi-code"],
        &[".kimi-code/mcp.json"],
        Entry::Plain,
        Some(SHARED_SKILLS),
    ),
    json(
        "Qwen Code",
        &[".qwen"],
        &[".qwen/settings.json"],
        Entry::Plain,
        Some(SHARED_SKILLS),
    ),
    Client {
        name: "Kilo",
        dirs: &[".config/kilo"],
        files: &[".config/kilo/kilo.json", ".config/kilo/kilo.jsonc"],
        how: How::Json {
            keys: &["mcp"],
            entry: Entry::List,
        },
        skills: Some(SHARED_SKILLS),
    },
    json(
        "LM Studio",
        &[".lmstudio", ".cache/lm-studio"],
        &[".lmstudio/mcp.json", ".cache/lm-studio/mcp.json"],
        Entry::Plain,
        None,
    ),
];

impl Client {
    /// The settings file that lists the agent's MCP servers.
    fn config(&self, home: &Path) -> PathBuf {
        let all = || self.files.iter().map(|f| home.join(f));
        all()
            .find(|f| f.is_file())
            .or_else(|| all().find(|f| f.parent().is_some_and(Path::is_dir)))
            .unwrap_or_else(|| home.join(self.files[0]))
    }

    /// Where the guide goes, for the agents that read skills.
    pub fn skill_path(&self, home: &Path) -> Option<PathBuf> {
        Some(home.join(self.skills?).join(NAME).join("SKILL.md"))
    }

    /// Whether this agent is on the computer.
    pub fn found(&self, home: &Path) -> bool {
        self.dirs.iter().any(|d| home.join(d).is_dir())
    }

    /// Whether pdfr is already in this agent's settings.
    pub fn installed(&self, home: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(self.config(home)) else {
            return false;
        };
        let in_json = |keys: &[&str]| {
            serde_json::from_str::<Value>(&text).is_ok_and(|v| {
                keys.iter()
                    .try_fold(&v, |v, k| v.get(k))
                    .is_some_and(|servers| servers.get(NAME).is_some())
            })
        };
        match self.how {
            How::ClaudeCode => in_json(&["mcpServers"]),
            How::Json { keys, .. } => in_json(keys),
            How::Opencode => in_json(&["mcp", "servers"]) || in_json(&["mcp"]),
            How::Toml => toml_has_server(&text),
            How::HermesYaml => hermes_has_server(&text),
            How::Crushrc => crushrc_has_server(&text),
            How::DshYaml => dsh_has_server(&text),
        }
    }

    /// Adds pdfr to this agent. May take a moment (Claude Code is asked
    /// through its own command), so call it off the UI thread.
    pub fn install(&self, home: &Path, launch: &Launch) -> Result<(), String> {
        if !self.installed(home) {
            let file = self.config(home);
            match self.how {
                How::ClaudeCode => claude_code_add(home, launch)?,
                How::Json { keys, entry } => json_add(&file, keys, entry_value(entry, launch))?,
                How::Opencode => opencode_add(&file, launch)?,
                How::Toml => append(&file, toml_has_server, &toml_table(launch))?,
                How::HermesYaml => hermes_add(&file, launch)?,
                How::Crushrc => append(&file, crushrc_has_server, &crushrc_line(launch))?,
                How::DshYaml => append(&file, dsh_has_server, &dsh_patch(launch))?,
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

/// A text settings file, empty if it is not there yet.
fn read_or_empty(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(said("read", path, e)),
    }
}

/// A JSON settings file as an object, empty if it is not there yet. A file
/// that is not plain JSON (comments, say) is an error: it is left alone.
fn read_json(path: &Path) -> Result<Map<String, Value>, String> {
    let text = read_or_empty(path)?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(root)) => Ok(root),
        Ok(_) => Err(said("read", path, "it is not a settings file")),
        Err(e) => Err(said(
            "read",
            path,
            format!("{e} (a file with comments has to be edited by hand)"),
        )),
    }
}

fn write_json(path: &Path, root: Map<String, Value>) -> Result<(), String> {
    let mut text =
        serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| said("write", path, e))?;
    text.push('\n');
    replace_file(path, &text)
}

/// The object at `keys` under `root`, made where it is missing.
fn object_at<'a>(
    mut at: &'a mut Map<String, Value>,
    keys: &[&str],
    path: &Path,
) -> Result<&'a mut Map<String, Value>, String> {
    for key in keys {
        at = at
            .entry(*key)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| {
                said(
                    "read",
                    path,
                    format!("its \"{key}\" is not a list of servers"),
                )
            })?;
    }
    Ok(at)
}

fn entry_value(entry: Entry, launch: &Launch) -> Value {
    match entry {
        Entry::Plain => json!({"command": launch.cmd, "args": launch.args}),
        Entry::Stdio => json!({"type": "stdio", "command": launch.cmd, "args": launch.args}),
        Entry::Local => json!({
            "type": "local", "command": launch.cmd, "args": launch.args, "tools": ["*"],
        }),
        Entry::List => json!({"type": "local", "command": command_list(launch), "enabled": true}),
    }
}

/// Adds pdfr to the servers at `keys` in a JSON settings file, keeping
/// everything else in it.
fn json_add(path: &Path, keys: &[&str], entry: Value) -> Result<(), String> {
    let mut root = read_json(path)?;
    object_at(&mut root, keys, path)?.insert(NAME.into(), entry);
    write_json(path, root)
}

/// opencode lists a server's command and arguments as one list. Version 2
/// keeps servers under `mcp.servers`; a file that already has servers right
/// under `mcp` was written for version 1 and gets pdfr there too.
fn opencode_add(path: &Path, launch: &Launch) -> Result<(), String> {
    let mut root = read_json(path)?;
    let old = root.get("mcp").and_then(Value::as_object).is_some_and(|m| {
        !m.contains_key("servers")
            && m.values()
                .any(|s| s.get("type").is_some() || s.get("command").is_some())
    });
    let keys: &[&str] = if old { &["mcp"] } else { &["mcp", "servers"] };
    object_at(&mut root, keys, path)?.insert(
        NAME.into(),
        json!({"type": "local", "command": command_list(launch)}),
    );
    write_json(path, root)
}

/// The program and its arguments as one list.
fn command_list(launch: &Launch) -> Vec<String> {
    let mut command = vec![launch.cmd.clone()];
    command.extend(launch.args.iter().cloned());
    command
}

/// Adds a block of text at the end of a settings file, unless `has` finds
/// pdfr in it already.
fn append(path: &Path, has: fn(&str) -> bool, block: &str) -> Result<(), String> {
    let mut text = read_or_empty(path)?;
    if has(&text) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(block);
    replace_file(path, &text)
}

fn toml_has_server(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim();
        l == "[mcp_servers.pdfr]" || l == "[mcp_servers.\"pdfr\"]"
    })
}

/// The lines of pdfr's entry under Hermes's `mcp_servers:`, indented by
/// `indent`. JSON strings and lists are valid YAML.
fn hermes_entry(indent: &str, launch: &Launch) -> String {
    format!(
        "{indent}{NAME}:\n{indent}{indent}command: {}\n{indent}{indent}args: {}\n",
        json!(launch.cmd),
        json!(launch.args),
    )
}

/// Where a YAML file's top-level `mcp_servers:` list starts (the line after
/// the key), and how its entries are indented.
fn hermes_servers(text: &str) -> Option<(usize, String)> {
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        at += line.len();
        if line.trim_end() == "mcp_servers:" {
            let next = text[at..].lines().find(|l| !l.trim().is_empty());
            let indent: String = next
                .map(|l| l.chars().take_while(|c| *c == ' ').collect())
                .unwrap_or_default();
            let indent = if indent.is_empty() {
                "  ".into()
            } else {
                indent
            };
            return Some((at, indent));
        }
    }
    None
}

fn hermes_has_server(text: &str) -> bool {
    hermes_servers(text).is_some_and(|(at, indent)| {
        text[at..]
            .lines()
            .take_while(|l| l.trim().is_empty() || l.starts_with(' ') || l.starts_with('#'))
            .any(|l| l.trim_end() == format!("{indent}{NAME}:"))
    })
}

/// Adds pdfr under `mcp_servers:` in Hermes's `config.yaml`: as the first
/// entry of the list that is there, or as a new list at the end. A list
/// written on one line (`mcp_servers: {…}`) is left for the user.
fn hermes_add(path: &Path, launch: &Launch) -> Result<(), String> {
    let text = read_or_empty(path)?;
    if hermes_has_server(&text) {
        return Ok(());
    }
    if let Some((at, indent)) = hermes_servers(&text) {
        let mut out = text[..at].to_string();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&hermes_entry(&indent, launch));
        out.push_str(&text[at..]);
        return replace_file(path, &out);
    }
    if text.lines().any(|l| l.starts_with("mcp_servers:")) {
        return Err(said(
            "change",
            path,
            "its mcp_servers are written on one line; add pdfr there by hand",
        ));
    }
    let block = format!("mcp_servers:\n{}", hermes_entry("  ", launch));
    append(path, |_| false, &block)
}

/// A table for Codex's `config.toml`. A new table at the end is valid
/// whatever comes before it, so the file need not be parsed. A JSON string
/// is a valid TOML string, and a JSON list of strings a TOML array.
fn toml_table(launch: &Launch) -> String {
    format!(
        "[mcp_servers.{NAME}]\ncommand = {}\nargs = {}\n",
        json!(launch.cmd),
        json!(launch.args),
    )
}

fn crushrc_has_server(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim_start().starts_with("mcp add pdfr "))
}

/// A word for a shell-like file: quoted unless it is plainly safe.
fn shell_word(s: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "-_./=:@".contains(c);
    if !s.is_empty() && s.chars().all(plain) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Crush's `crushrc` is a list of commands: `mcp add` registers a server.
fn crushrc_line(launch: &Launch) -> String {
    let mut line = format!("mcp add {NAME} --command {}", shell_word(&launch.cmd));
    for a in &launch.args {
        line.push_str(&format!(" --args {}", shell_word(a)));
    }
    line.push('\n');
    line
}

fn dsh_has_server(text: &str) -> bool {
    text.lines().any(|l| l.trim() == "serverName: pdfr")
}

/// DeepSeek Harness reads a YAML list of patches; one more item at the end
/// adds an MCP client for pdfr. JSON strings and lists are valid YAML.
fn dsh_patch(launch: &Launch) -> String {
    format!(
        "- insert:\n    - id: {NAME}\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        \
         serverName: {NAME}\n        transport: stdio\n        command: {}\n        args: {}\n        \
         env: {{}}\n        cwd: !!js process.cwd()\n",
        json!(launch.cmd),
        json!(launch.args),
    )
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
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("pdfr-install-{}-{n}-{test}", std::process::id()));
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

    fn client(name: &str) -> &'static Client {
        CLIENTS.iter().find(|c| c.name == name).unwrap()
    }

    /// A home folder where `name` is installed, with `text` as its settings
    /// (`None`: no settings file yet). Gives the home and the file.
    fn with(name: &str, text: Option<&str>) -> (PathBuf, PathBuf) {
        let c = client(name);
        let home = home_for(&name.replace(' ', "-"));
        std::fs::create_dir_all(home.join(c.dirs[0])).unwrap();
        let file = home.join(c.files[0]);
        if let Some(text) = text {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, text).unwrap();
        }
        (home, file)
    }

    fn read_json_file(file: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    #[test]
    fn only_agents_on_the_computer_are_found() {
        let home = home_for("found");
        assert!(CLIENTS.iter().all(|c| !c.found(&home)));
        std::fs::create_dir_all(home.join(".cursor")).unwrap();
        assert!(client("Cursor").found(&home));
        assert!(!client("Codex and ChatGPT Desktop").found(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Every agent but Claude Code (asked through its own command) can be
    /// installed into an empty settings folder, is then seen as installed,
    /// and a second install changes nothing.
    #[test]
    fn every_agent_installs_once() {
        for c in CLIENTS.iter().filter(|c| c.how != How::ClaudeCode) {
            let (home, _) = with(c.name, None);
            assert!(c.found(&home) && !c.installed(&home), "{}", c.name);
            c.install(&home, &launch()).unwrap();
            assert!(c.installed(&home), "{}", c.name);
            let file = c.config(&home);
            let first = std::fs::read_to_string(&file).unwrap();
            assert!(first.contains("/opt/My Apps/pdfr"), "{}", c.name);
            c.install(&home, &launch()).unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap(), first, "{}", c.name);
            // The guide is where the agent reads skills.
            if let Some(skill) = c.skill_path(&home) {
                assert_eq!(std::fs::read_to_string(skill).unwrap(), SKILL, "{}", c.name);
            }
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn cursor_keeps_its_other_servers() {
        let (home, file) = with(
            "Cursor",
            Some(r#"{"mcpServers": {"other": {"command": "x"}}, "theme": "dark"}"#),
        );
        client("Cursor").install(&home, &launch()).unwrap();
        let v = read_json_file(&file);
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["theme"], "dark");
        assert_eq!(
            v["mcpServers"]["pdfr"],
            json!({"type": "stdio", "command": "/opt/My Apps/pdfr", "args": ["mcp"]})
        );
        // The guide goes to the folder most agents share.
        assert!(home.join(".agents/skills/pdfr/SKILL.md").is_file());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_settings_file_that_cannot_be_read_is_left_alone() {
        for text in ["{ not json", "// mine\n{\"mcpServers\": {}}", "[1, 2]"] {
            let (home, file) = with("Cursor", Some(text));
            assert!(client("Cursor").install(&home, &launch()).is_err());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn each_agent_gets_its_own_shape() {
        let (home, file) = with("GitHub Copilot", None);
        client("GitHub Copilot").install(&home, &launch()).unwrap();
        let v = read_json_file(&file);
        assert_eq!(v["mcpServers"]["pdfr"]["type"], "local");
        assert_eq!(v["mcpServers"]["pdfr"]["tools"], json!(["*"]));
        let _ = std::fs::remove_dir_all(&home);

        let (home, file) = with("Kilo", None);
        client("Kilo").install(&home, &launch()).unwrap();
        assert_eq!(
            read_json_file(&file)["mcp"]["pdfr"],
            json!({"type": "local", "command": ["/opt/My Apps/pdfr", "mcp"], "enabled": true})
        );
        let _ = std::fs::remove_dir_all(&home);

        let (home, file) = with("OpenClaw", None);
        client("OpenClaw").install(&home, &launch()).unwrap();
        assert_eq!(
            read_json_file(&file)["mcp"]["servers"]["pdfr"]["args"],
            json!(["mcp"])
        );
        let _ = std::fs::remove_dir_all(&home);

        // Amp's servers are under one dotted key, next to its other settings.
        let (home, file) = with("Amp", Some(r#"{"amp.notifications.enabled": true}"#));
        client("Amp").install(&home, &launch()).unwrap();
        let v = read_json_file(&file);
        assert_eq!(v["amp.notifications.enabled"], true);
        assert_eq!(v["amp.mcpServers"]["pdfr"]["args"], json!(["mcp"]));
        let _ = std::fs::remove_dir_all(&home);

        let (home, file) = with("Crush", Some("permissions allow view"));
        client("Crush").install(&home, &launch()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "permissions allow view\n\nmcp add pdfr --command '/opt/My Apps/pdfr' --args mcp\n"
        );
        let _ = std::fs::remove_dir_all(&home);

        let (home, file) = with("DeepSeek Harness", None);
        client("DeepSeek Harness")
            .install(&home, &launch())
            .unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.starts_with("- insert:\n    - id: pdfr\n"));
        assert!(text.contains("        command: \"/opt/My Apps/pdfr\"\n        args: [\"mcp\"]\n"));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn hermes_joins_the_servers_that_are_there() {
        // No servers yet: a new list at the end.
        let (home, file) = with("Hermes", Some("model: x\n"));
        client("Hermes").install(&home, &launch()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "model: x\n\nmcp_servers:\n  pdfr:\n    command: \"/opt/My Apps/pdfr\"\n    args: [\"mcp\"]\n"
        );
        let _ = std::fs::remove_dir_all(&home);
        // A list is there: pdfr goes first in it, indented like the rest.
        let before = "mcp_servers:\n    other:\n        command: y\nmodel: x\n";
        let (home, file) = with("Hermes", Some(before));
        assert!(!client("Hermes").installed(&home));
        client("Hermes").install(&home, &launch()).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "mcp_servers:\n    pdfr:\n        command: \"/opt/My Apps/pdfr\"\n        \
             args: [\"mcp\"]\n    other:\n        command: y\nmodel: x\n"
        );
        assert!(client("Hermes").installed(&home));
        let _ = std::fs::remove_dir_all(&home);
        // Written on one line: left alone.
        let (home, file) = with("Hermes", Some("mcp_servers: {}\n"));
        assert!(client("Hermes").install(&home, &launch()).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mcp_servers: {}\n");
        let _ = std::fs::remove_dir_all(&home);
        // Another tool's "pdfr:" key elsewhere is not ours.
        let (home, _) = with(
            "Hermes",
            Some("mcp_servers:\n  a:\n    command: y\ntools:\n  pdfr:\n"),
        );
        assert!(!client("Hermes").installed(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn opencode_gets_the_shape_its_file_already_uses() {
        // Nothing there yet: version 2's `mcp.servers`.
        let (home, file) = with("OpenCode", Some(r#"{"theme": "x"}"#));
        client("OpenCode").install(&home, &launch()).unwrap();
        let v = read_json_file(&file);
        assert_eq!(
            v["mcp"]["servers"]["pdfr"],
            json!({"type": "local", "command": ["/opt/My Apps/pdfr", "mcp"]})
        );
        assert_eq!(v["theme"], "x");
        let _ = std::fs::remove_dir_all(&home);
        // Servers right under `mcp`: a version 1 file, and pdfr joins them.
        let (home, file) = with(
            "OpenCode",
            Some(r#"{"mcp": {"other": {"type": "local", "command": ["x"]}}}"#),
        );
        client("OpenCode").install(&home, &launch()).unwrap();
        let v = read_json_file(&file);
        assert_eq!(v["mcp"]["pdfr"]["command"][1], "mcp");
        assert!(v["mcp"].get("servers").is_none());
        assert!(client("OpenCode").installed(&home));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_settings_file_that_is_there_is_the_one_used() {
        // LM Studio keeps its file in one of two places.
        let c = client("LM Studio");
        let home = home_for("lmstudio-cache");
        std::fs::create_dir_all(home.join(".cache/lm-studio")).unwrap();
        assert!(c.found(&home));
        c.install(&home, &launch()).unwrap();
        assert!(home.join(".cache/lm-studio/mcp.json").is_file());
        assert!(!home.join(".lmstudio").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn codex_gets_a_table() {
        let (home, file) = with(
            "Codex and ChatGPT Desktop",
            Some("model = \"x\"\n\n[mcp_servers.other]\ncommand = \"y\""),
        );
        client("Codex and ChatGPT Desktop")
            .install(&home, &launch())
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "model = \"x\"\n\n[mcp_servers.other]\ncommand = \"y\"\n\n\
             [mcp_servers.pdfr]\ncommand = \"/opt/My Apps/pdfr\"\nargs = [\"mcp\"]\n"
        );
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
        let (home, file) = with("Cursor", None);
        let real = home.join("dotfiles-mcp.json");
        std::fs::write(&real, "{}").unwrap();
        std::os::unix::fs::symlink(&real, &file).unwrap();
        client("Cursor").install(&home, &launch()).unwrap();
        assert!(file.is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("pdfr"));
        let _ = std::fs::remove_dir_all(&home);
    }
}
