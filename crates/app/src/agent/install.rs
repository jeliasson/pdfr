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

/// The start of the line that marks a guide pdfr wrote (see `SKILL.md`):
/// a later install may replace such a file.
const SKILL_MARK: &str = "<!-- pdfr-skill ";

/// FNV-1a hashes of the guides earlier versions of pdfr wrote, before they
/// carried the mark: such a file is pdfr's own, unchanged, and may be
/// replaced too.
const OLD_SKILLS: &[u64] = &[
    // pdfr 0.5 and 0.6.
    0x4602_6730_f8b3_b431,
];

/// Where pdfr's guide goes when the user has a SKILL.md of their own there.
const SKILL_NEW: &str = "SKILL.pdfr-new.md";

/// The command an agent runs to reach pdfr: the program and its arguments
/// (ending in `mcp`).
#[derive(Clone, Debug, PartialEq)]
pub struct Launch {
    pub cmd: String,
    pub args: Vec<String>,
}

impl Launch {
    /// How agents start this copy of pdfr: through Flatpak, as `pdfr` when
    /// it is on the PATH, otherwise by the program's full path.
    pub fn this_copy(flatpak: bool) -> Launch {
        let on_path = pdfr_assist::ai::which("pdfr").is_some();
        Launch::from_parts(flatpak, on_path, std::env::current_exe().ok())
    }

    /// The command for each case. A path is one argument, spaces and all.
    fn from_parts(flatpak: bool, on_path: bool, exe: Option<PathBuf>) -> Launch {
        let mcp = "mcp".to_string();
        if flatpak {
            return Launch {
                cmd: "flatpak".into(),
                args: vec![
                    "run".into(),
                    "--command=pdfr".into(),
                    crate::system::APP_ID.into(),
                    mcp,
                ],
            };
        }
        let cmd = match exe {
            // Not on the PATH (run from an archive or an app bundle).
            Some(p) if !on_path => p.display().to_string(),
            _ => "pdfr".into(),
        };
        Launch {
            cmd,
            args: vec![mcp],
        }
    }

    /// The command as a shell line, each word quoted where it needs to be.
    pub fn shell_line(&self) -> String {
        std::iter::once(&self.cmd)
            .chain(&self.args)
            .map(|w| shell_word(w))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// How an agent's entry for a server is written in a JSON settings file.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Entry {
    /// `{"command": …, "args": […]}`
    Plain,
    /// The same with `"type": "stdio"`.
    Stdio,
    /// GitHub Copilot: `"type": "local"` and only the read-only tools
    /// allowed without asking.
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
    /// through its own command), so call it off the UI thread. Gives a note
    /// for the user when something was not done the usual way.
    pub fn install(&self, home: &Path, launch: &Launch) -> Result<Option<String>, String> {
        if !self.installed(home) {
            let file = self.config(home);
            match self.how {
                How::ClaudeCode => claude_code_add(home, launch)?,
                How::Json { keys, entry } => json_add(&file, keys, entry_value(entry, launch))?,
                How::Opencode => opencode_add(&file, launch)?,
                How::Toml => toml_add(&file, launch)?,
                How::HermesYaml => hermes_add(&file, launch)?,
                How::Crushrc => append(&file, crushrc_has_server, &crushrc_line(launch))?,
                How::DshYaml => append(&file, dsh_has_server, &dsh_patch(launch))?,
            }
        }
        match self.skill_path(home) {
            Some(path) => write_skill(&path),
            None => Ok(None),
        }
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

/// FNV-1a: a stable hash, to recognise pdfr's earlier guides.
fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Whether a SKILL.md is pdfr's own: it carries the mark, or it is a guide
/// an earlier pdfr wrote, unchanged.
fn skill_is_ours(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().starts_with(SKILL_MARK))
        || OLD_SKILLS.contains(&fnv(text.replace("\r\n", "\n").as_bytes()))
}

/// Writes the guide, replacing an older copy of it. A SKILL.md the user
/// wrote or changed is kept: the guide goes next to it, and the note says
/// so.
fn write_skill(path: &Path) -> Result<Option<String>, String> {
    let old = match std::fs::read(path) {
        Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(said("read", path, e)),
    };
    match old {
        Some(text) if text == SKILL => Ok(None),
        Some(text) if !skill_is_ours(&text) => {
            let new = path.with_file_name(SKILL_NEW);
            replace_file(&new, SKILL)?;
            Ok(Some(format!(
                "{} has changes of your own, so it was kept. pdfr's guide is in {SKILL_NEW} next to it.",
                path.display(),
            )))
        }
        _ => replace_file(path, SKILL).map(|()| None),
    }
}

/// Replaces a file in one step, so a crash half way leaves the old one. A
/// symlinked file (dotfiles) stays a symlink: its target is replaced. The
/// file keeps its permissions; a new one is readable by the user only.
fn replace_file(path: &Path, text: &str) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = target.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| said("create", dir, e))?;
    }
    pdfr_core::fsx::write_atomic(path, text.as_bytes()).map_err(|e| said("write", path, e))
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
            "type": "local", "command": launch.cmd, "args": launch.args,
            "tools": super::mcp::READ_ONLY,
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

/// The servers in a Codex `config.toml`, however they are written: a
/// `[mcp_servers]` table, `[mcp_servers.<name>]` tables, dotted keys or an
/// inline table.
fn toml_servers(doc: &toml_edit::DocumentMut) -> Option<&dyn toml_edit::TableLike> {
    doc.get("mcp_servers")?.as_table_like()
}

fn toml_has_server(text: &str) -> bool {
    text.parse::<toml_edit::DocumentMut>()
        .is_ok_and(|doc| toml_servers(&doc).is_some_and(|t| t.contains_key(NAME)))
}

/// Adds pdfr to the servers in a Codex `config.toml`: a new
/// `[mcp_servers.pdfr]` table at the end, or an entry in the inline table
/// the servers are written as. Comments and layout are kept.
fn toml_add(path: &Path, launch: &Launch) -> Result<(), String> {
    use toml_edit::{Array, InlineTable, Item, Table, Value as TomlValue, value};
    let text = read_or_empty(path)?;
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| {
        said(
            "read",
            path,
            format!("it is not valid TOML ({})", e.message()),
        )
    })?;
    if toml_servers(&doc).is_some_and(|t| t.contains_key(NAME)) {
        return Ok(());
    }
    let args: Array = launch.args.iter().map(String::as_str).collect();
    let root = doc.as_table_mut();
    if !root.contains_key("mcp_servers") {
        let mut servers = Table::new();
        servers.set_implicit(true);
        root.insert("mcp_servers", Item::Table(servers));
    }
    match root.get_mut("mcp_servers") {
        Some(Item::Table(servers)) => {
            let mut t = Table::new();
            if !text.trim().is_empty() {
                t.decor_mut().set_prefix("\n");
            }
            t.insert("command", value(launch.cmd.as_str()));
            t.insert("args", value(args));
            servers.insert(NAME, Item::Table(t));
        }
        Some(Item::Value(TomlValue::InlineTable(servers))) => {
            let mut t = InlineTable::new();
            t.insert("command", launch.cmd.as_str().into());
            t.insert("args", TomlValue::Array(args));
            servers.insert(NAME, TomlValue::InlineTable(t));
        }
        _ => {
            return Err(said(
                "change",
                path,
                "its mcp_servers is not a table; add pdfr there by hand",
            ));
        }
    }
    let mut out = doc.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    replace_file(path, &out)
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
        // A YAML list (`- name: …`) is not the shape Hermes reads: a named
        // entry added to it would break the file.
        let first = text[at..]
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty() && !l.starts_with('#'));
        if first.is_some_and(|l| l == "-" || l.starts_with("- ")) {
            return Err(said(
                "change",
                path,
                "its mcp_servers are written as a list (lines starting with \"-\"), not as \
                 named entries; add pdfr there by hand",
            ));
        }
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
        // Only the tools that change nothing run without Copilot asking.
        assert_eq!(
            v["mcpServers"]["pdfr"]["tools"],
            json!(super::super::mcp::READ_ONLY)
        );
        assert!(!v.to_string().contains("\"*\""));
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

    /// pdfr in Codex's settings is found however it is written, and is
    /// then not added a second time.
    #[test]
    fn codex_finds_pdfr_written_any_way() {
        for text in [
            "[mcp_servers.pdfr]\ncommand = \"x\"\n",
            "[mcp_servers.\"pdfr\"]\ncommand = \"x\"\n",
            "[mcp_servers]\npdfr = { command = \"x\" }\n",
            "mcp_servers.pdfr.command = \"x\"\n",
            "mcp_servers = { pdfr = { command = \"x\" } }\n",
            "# servers\n[ mcp_servers . pdfr ] # mine\ncommand = \"x\"\n",
        ] {
            let (home, file) = with("Codex and ChatGPT Desktop", Some(text));
            let c = client("Codex and ChatGPT Desktop");
            assert!(c.installed(&home), "{text}");
            c.install(&home, &launch()).unwrap();
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
            let _ = std::fs::remove_dir_all(&home);
        }
        // A key of another table named pdfr is not ours.
        assert!(!toml_has_server("[tools]\npdfr = 1\n"));
    }

    #[test]
    fn codex_servers_in_an_inline_table_get_pdfr_there() {
        let text = "# my settings\nmcp_servers = { other = { command = \"y\" } }\nmodel = \"x\"\n";
        let (home, file) = with("Codex and ChatGPT Desktop", Some(text));
        let c = client("Codex and ChatGPT Desktop");
        assert!(!c.installed(&home));
        c.install(&home, &launch()).unwrap();
        let out = std::fs::read_to_string(&file).unwrap();
        assert!(out.starts_with("# my settings\n"), "{out}");
        assert!(c.installed(&home), "{out}");
        let doc: toml_edit::DocumentMut = out.parse().unwrap();
        assert_eq!(
            doc["mcp_servers"]["pdfr"]["command"].as_str(),
            Some("/opt/My Apps/pdfr")
        );
        assert_eq!(doc["mcp_servers"]["other"]["command"].as_str(), Some("y"));
        assert_eq!(doc["model"].as_str(), Some("x"));
        let _ = std::fs::remove_dir_all(&home);
        // TOML that does not parse is left alone.
        let (home, file) = with("Codex and ChatGPT Desktop", Some("model = \n"));
        assert!(c.install(&home, &launch()).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "model = \n");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn hermes_servers_written_as_a_list_are_left_alone() {
        for before in [
            "mcp_servers:\n  - name: other\n    command: y\n",
            "mcp_servers:\n# mine\n- name: other\n",
        ] {
            let (home, file) = with("Hermes", Some(before));
            let e = client("Hermes").install(&home, &launch()).unwrap_err();
            assert!(e.contains("list"), "{e}");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn a_rewritten_json_file_keeps_its_key_order() {
        let text = r#"{"zeta": 1, "mcpServers": {"yak": {"command": "y"}, "ant": {"command": "a"}}, "alpha": 2}"#;
        let (home, file) = with("Cursor", Some(text));
        client("Cursor").install(&home, &launch()).unwrap();
        let out = std::fs::read_to_string(&file).unwrap();
        let at = |k: &str| out.find(&format!("\"{k}\"")).unwrap();
        assert!(at("zeta") < at("mcpServers") && at("mcpServers") < at("alpha"));
        assert!(at("yak") < at("ant") && at("ant") < at("pdfr"), "{out}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[cfg(unix)]
    #[test]
    fn a_rewritten_settings_file_keeps_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (home, file) = with("Cursor", Some("{}"));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        client("Cursor").install(&home, &launch()).unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        // A new file is the user's alone.
        let (home2, file2) = with("Kiro", None);
        client("Kiro").install(&home2, &launch()).unwrap();
        let mode = std::fs::metadata(&file2).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        // No temporary file is left behind.
        let left: Vec<_> = std::fs::read_dir(file.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, vec![std::ffi::OsString::from("mcp.json")]);
        let _ = std::fs::remove_dir_all(&home);
        let _ = std::fs::remove_dir_all(&home2);
    }

    #[test]
    fn a_path_with_spaces_stays_one_word() {
        let exe = PathBuf::from("/home/me/My Apps/pdfr's copy/pdfr");
        let l = Launch::from_parts(false, false, Some(exe.clone()));
        assert_eq!(l.cmd, exe.display().to_string());
        assert_eq!(l.args, vec!["mcp"]);
        assert_eq!(
            l.shell_line(),
            r#"'/home/me/My Apps/pdfr'\''s copy/pdfr' mcp"#
        );
        // On the PATH: just its name.
        let l = Launch::from_parts(false, true, Some(exe));
        assert_eq!(
            (l.cmd.as_str(), l.shell_line().as_str()),
            ("pdfr", "pdfr mcp")
        );
        // Flatpak: flatpak runs it.
        let l = Launch::from_parts(true, false, None);
        assert_eq!(l.cmd, "flatpak");
        assert_eq!(
            l.args,
            vec!["run", "--command=pdfr", crate::system::APP_ID, "mcp"]
        );
    }

    #[test]
    fn a_guide_the_user_changed_is_kept() {
        let c = client("Cursor");
        let (home, _) = with("Cursor", None);
        let skill = c.skill_path(&home).unwrap();
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        // The user's own guide: kept, pdfr's goes next to it.
        std::fs::write(&skill, "my own notes on pdfr\n").unwrap();
        let note = c.install(&home, &launch()).unwrap().expect("a note");
        assert!(note.contains(SKILL_NEW), "{note}");
        assert_eq!(
            std::fs::read_to_string(&skill).unwrap(),
            "my own notes on pdfr\n"
        );
        let new = skill.with_file_name(SKILL_NEW);
        assert_eq!(std::fs::read_to_string(&new).unwrap(), SKILL);
        // A guide with pdfr's mark is replaced, whatever it says.
        std::fs::write(&skill, "---\n<!-- pdfr-skill v1 -->\nold words\n").unwrap();
        assert_eq!(c.install(&home, &launch()).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), SKILL);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// pdfr 0.6 wrote the guide without the mark: that file, unchanged, is
    /// still replaced.
    #[test]
    fn the_guide_of_an_earlier_pdfr_is_replaced() {
        let mark = SKILL
            .lines()
            .find(|l| l.starts_with(SKILL_MARK))
            .expect("the guide carries the mark");
        let old = SKILL.replacen(&format!("{mark}\n\n"), "", 1);
        assert!(skill_is_ours(&old) || fnv(old.as_bytes()) != OLD_SKILLS[0]);
        if fnv(old.as_bytes()) != OLD_SKILLS[0] {
            // The guide has changed since 0.6: nothing more to show here.
            return;
        }
        let c = client("Cursor");
        let (home, _) = with("Cursor", None);
        let skill = c.skill_path(&home).unwrap();
        std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
        std::fs::write(&skill, &old).unwrap();
        assert_eq!(c.install(&home, &launch()).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&skill).unwrap(), SKILL);
        assert!(!skill.with_file_name(SKILL_NEW).exists());
        // One word changed: the user's.
        std::fs::write(&skill, old.replacen("pdfr", "PDFR", 1)).unwrap();
        assert!(c.install(&home, &launch()).unwrap().is_some());
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
