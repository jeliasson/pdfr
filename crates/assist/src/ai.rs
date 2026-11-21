//! Layer 2's last resort: ask an AI tool the user has (a command-line agent,
//! or a model server running on this computer) to map unknown labels to
//! canonical keys.
//!
//! A tool is offered only if it answers as a plain model, with every tool of
//! its own switched off: a label printed in a PDF is text someone else wrote,
//! and an agent that can read files or run commands could be talked into
//! reading and sending what pdfr remembers about the user. Claude Code, Pi,
//! MiniMax Code and Kimi Code can be run that way; Ollama and LM Studio are
//! bare model servers. Agents that cannot (Codex, Cursor, Antigravity, Grok,
//! Hermes, …) are not used here; they can still drive pdfr over MCP.
//!
//! Token rules: labels only (never values, page text, images or the PDF), one
//! call per document (split above 150 labels), closed vocabulary, compact
//! numbered request and `index:key` reply, no tools, at most one retry.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const MAX_LABEL_CHARS: usize = 80;
pub const MAX_LABELS_PER_CALL: usize = 150;
pub const TIMEOUT: Duration = Duration::from_secs(90);

/// The fixed instruction. Kept short: it is sent with every call.
pub const INSTRUCTION: &str = "Map each numbered form-field label to one key from KEYS. \
Reply only with one line per label, number colon key, e.g. `1:email`; no other text. Use `none` if the field is not a reusable personal or company detail \
(signatures, amounts, free text, other people's details). Use `new:snake_case` only if no key fits. \
A name field asking for family name first: append `|ff`. Date part fields: append `.day`, `.month` or `.year`.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Key(String),
    None,
    New(String),
}

/// Raw backend reply plus token usage, when the tool reports it.
#[derive(Clone, Debug, Default)]
pub struct Reply {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

pub trait Backend: Send + Sync {
    /// Human-readable tool name for the consent dialog ("Claude Code (claude)").
    fn name(&self) -> String;
    fn run(&self, instruction: &str, prompt: &str) -> Result<Reply, String>;
}

/// Shortens and tidies a label for sending.
pub fn prepare_label(label: &str) -> String {
    let s = label.split_whitespace().collect::<Vec<_>>().join(" ");
    s.chars().take(MAX_LABEL_CHARS).collect()
}

/// The request text: vocabulary and numbered labels. Nothing else.
pub fn build_prompt(labels: &[String], vocab: &[String]) -> String {
    let mut s = String::with_capacity(64 + labels.len() * 24 + vocab.len() * 12);
    s.push_str("KEYS: ");
    s.push_str(&vocab.join(","));
    s.push_str("\nLABELS:\n");
    for (i, l) in labels.iter().enumerate() {
        s.push_str(&format!("{}:{}\n", i + 1, prepare_label(l)));
    }
    s
}

fn snake(s: &str) -> Option<String> {
    let t = s.trim().to_lowercase();
    let ok = !t.is_empty()
        && t.len() <= 40
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    ok.then_some(t)
}

/// Parses `n:key` lines (or a flat JSON array of keys). Returns `None` if the
/// reply does not parse: it is then discarded, never retried in a loop.
pub fn parse_reply(reply: &str, n: usize, vocab: &[String]) -> Option<Vec<Option<Answer>>> {
    let reply = reply
        .trim()
        .trim_start_matches("```")
        .trim_start_matches("text")
        .trim_end_matches("```")
        .trim();
    let mut out: Vec<Option<Answer>> = vec![None; n];
    let parse_key = |raw: &str| -> Option<Answer> {
        let raw = raw
            .trim()
            .trim_matches(|c| c == '"' || c == '`' || c == ',');
        let raw = raw.strip_prefix("key:").unwrap_or(raw).trim();
        if raw.eq_ignore_ascii_case("none") {
            return Some(Answer::None);
        }
        if let Some(k) = raw.strip_prefix("new:") {
            return snake(k).map(Answer::New);
        }
        // Accept suffixes `.day` / `|ff` on known keys.
        let base = raw.split(['|', '.']).next().unwrap_or(raw);
        if vocab.iter().any(|v| v == base) {
            let suffix_ok = raw[base.len()..]
                .split(['|', '.'])
                .all(|s| ["", "ff", "day", "month", "year"].contains(&s));
            return suffix_ok.then(|| Answer::Key(raw.to_string()));
        }
        None
    };
    if reply.starts_with('[') {
        let arr: Vec<String> = serde_json::from_str(reply).ok()?;
        if arr.len() != n {
            return None;
        }
        for (i, k) in arr.iter().enumerate() {
            out[i] = Some(parse_key(k)?);
        }
        return Some(out);
    }
    for line in reply.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (idx, key) = line.split_once(':')?;
        let idx: usize = idx.trim().trim_start_matches('#').parse().ok()?;
        // Every label answered exactly once; anything else is discarded, so
        // a partial reply never caches "not reusable" for unanswered labels.
        if idx == 0 || idx > n || out[idx - 1].is_some() {
            return None;
        }
        out[idx - 1] = Some(parse_key(key)?);
    }
    out.iter().all(Option::is_some).then_some(out)
}

// ----------------------------------------------------------------- CLI tools

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Claude,
    /// Kept for its reply parser; never chosen (see `TOOLS`).
    Codex,
    Pi,
    MiniMax,
    Kimi,
    Ollama,
    LmStudio,
}

/// The tools pdfr may use, in the order it looks for one.
pub const TOOLS: [Tool; 6] = [
    Tool::Claude,
    Tool::Pi,
    Tool::MiniMax,
    Tool::Kimi,
    Tool::Ollama,
    Tool::LmStudio,
];

impl Tool {
    /// The name kept in the settings file.
    pub fn id(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Pi => "pi",
            Tool::MiniMax => "minimax",
            Tool::Kimi => "kimi",
            Tool::Ollama => "ollama",
            Tool::LmStudio => "lmstudio",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        TOOLS.into_iter().find(|t| t.id() == id)
    }

    /// The name shown to the user.
    pub fn name(self) -> &'static str {
        match self {
            Tool::Claude => "Claude Code",
            Tool::Codex => "Codex",
            Tool::Pi => "Pi",
            Tool::MiniMax => "MiniMax Code",
            Tool::Kimi => "Kimi Code",
            Tool::Ollama => "Ollama",
            Tool::LmStudio => "LM Studio",
        }
    }

    /// The command of a command-line tool; empty for a model server.
    pub fn binary(self) -> &'static str {
        match self {
            Tool::Claude => "claude",
            Tool::Codex => "codex",
            Tool::Pi => "pi",
            Tool::MiniMax => "mcode",
            Tool::Kimi => "kimi",
            Tool::Ollama | Tool::LmStudio => "",
        }
    }

    /// The tool's own folder under the home folder. A command on the PATH
    /// can be a launcher that installs the tool when first run; the folder
    /// shows the user has run it.
    fn home_dir(self) -> Option<&'static str> {
        match self {
            Tool::Pi => Some(".pi/agent"),
            Tool::MiniMax => Some(".minimax"),
            Tool::Kimi => Some(".kimi-code"),
            _ => None,
        }
    }

    /// Where a model server listens on this computer (its OpenAI-style API).
    pub fn server(self) -> Option<(u16, &'static str)> {
        match self {
            Tool::Ollama => Some((11434, "http://127.0.0.1:11434/v1")),
            Tool::LmStudio => Some((1234, "http://127.0.0.1:1234/v1")),
            _ => None,
        }
    }

    /// The model used when the settings name none. Claude Code: its
    /// cheapest. Empty: the tool's own default (a model server: the first
    /// model it has).
    pub fn default_model(self) -> &'static str {
        match self {
            Tool::Claude => "haiku",
            Tool::Codex => "gpt-6-luna",
            _ => "",
        }
    }

    /// Whether the tool can be used right now: its command is installed and
    /// has been run, or its server is listening.
    pub fn available(self) -> bool {
        if let Some((port, _)) = self.server() {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
            return std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok();
        }
        which(self.binary()).is_some()
            && self.home_dir().is_none_or(|d| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .is_some_and(|h| PathBuf::from(h).join(d).is_dir())
            })
    }
}

/// Models offered in Settings for Claude Code: `(alias, name, note)`. The
/// CLI has no command that lists models; these aliases always point at the
/// latest model of each line. The first one is the default.
pub const CLAUDE_MODELS: &[(&str, &str, &str)] = &[
    ("haiku", "Haiku", "Fastest and cheapest"),
    ("sonnet", "Sonnet", "Balanced"),
    ("opus", "Opus", "Most capable"),
    ("fable", "Fable", "Newest line"),
];

pub struct CliBackend {
    pub tool: Tool,
    pub path: PathBuf,
    pub model: String,
}

/// Finds an executable on PATH.
pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or(".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for e in &exts {
            let p = dir.join(format!("{bin}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// The tool to use: the one chosen in the settings (`tool`, by id) if it
/// can be used now, else the first of `TOOLS` that can. `claude_model` is
/// the setting for Claude Code; `model` the one for the chosen tool.
///
/// Codex is not offered: its non-interactive mode has no way to turn its
/// shell tool off (even its read-only sandbox can read every file), so a
/// hostile label printed in a PDF could make it read and send the user's
/// remembered values. Every tool here runs with all tools disabled.
pub fn detect(tool: &str, claude_model: &str, model: &str) -> Option<std::sync::Arc<dyn Backend>> {
    let chosen = Tool::from_id(tool);
    let tool = match chosen {
        Some(t) if t.available() => t,
        // The chosen tool is gone: no other is used in its place, so labels
        // are never sent somewhere the user did not pick.
        Some(_) => return None,
        None => TOOLS.into_iter().find(|t| t.available())?,
    };
    let model = match tool {
        Tool::Claude => claude_model,
        // A model named for one tool means nothing to another.
        _ if chosen.is_some() => model,
        _ => "",
    };
    let model = Some(model.trim())
        .filter(|m| !m.is_empty())
        .unwrap_or(tool.default_model())
        .to_string();
    Some(match tool.server() {
        Some((_, base)) => std::sync::Arc::new(HttpBackend {
            tool,
            base: base.to_string(),
            model,
        }),
        None => std::sync::Arc::new(CliBackend {
            tool,
            path: which(tool.binary())?,
            model,
        }),
    })
}

/// A model server on this computer, spoken to in the OpenAI style
/// (`/chat/completions`). It is a bare model: it has no tools to switch off.
pub struct HttpBackend {
    pub tool: Tool,
    /// The API's address, ending in `/v1`.
    pub base: String,
    /// Empty: the first model the server lists.
    pub model: String,
}

fn http_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// The models a server has, as it lists them.
pub fn server_models(base: &str, timeout: Duration) -> Result<Vec<String>, String> {
    let body = http_agent(timeout)
        .get(format!("{base}/models"))
        .call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    Ok(v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// A chat reply in the OpenAI style: the text and the tokens used. What a
/// reasoning model wrote inside `<think>` is not part of the answer.
pub fn parse_chat_json(out: &str) -> Result<Reply, String> {
    let v: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("bad JSON from the server: {e}"))?;
    if let Some(e) = v.get("error") {
        return Err(e["message"].as_str().unwrap_or("error").to_string());
    }
    let mut text = v["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("no reply")?
        .to_string();
    while let Some(a) = text.find("<think>") {
        match text[a..].find("</think>") {
            Some(b) => text.replace_range(a..a + b + "</think>".len(), ""),
            None => text.truncate(a),
        }
    }
    Ok(Reply {
        text: text.trim().to_string(),
        input_tokens: v["usage"]["prompt_tokens"].as_u64(),
        output_tokens: v["usage"]["completion_tokens"].as_u64(),
    })
}

impl Backend for HttpBackend {
    fn name(&self) -> String {
        match self.model.as_str() {
            "" => format!("{} (on this computer)", self.tool.name()),
            m => format!("{} (on this computer, model {m})", self.tool.name()),
        }
    }

    fn run(&self, instruction: &str, prompt: &str) -> Result<Reply, String> {
        let model = match self.model.as_str() {
            "" => server_models(&self.base, Duration::from_secs(5))?
                .into_iter()
                .next()
                .ok_or_else(|| format!("{} has no model yet", self.tool.name()))?,
            m => m.to_string(),
        };
        let request = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": instruction},
                {"role": "user", "content": prompt},
            ],
            "temperature": 0,
            "stream": false,
        });
        let body = http_agent(TIMEOUT)
            .post(format!("{}/chat/completions", self.base))
            .header("Content-Type", "application/json")
            .send(request.to_string())
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        parse_chat_json(&body)
    }
}

/// A private, freshly created, randomly named empty directory for the tool
/// to run in, so it sees no project files. Removed with everything in it.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<TempDir, String> {
        use std::hash::{BuildHasher, Hasher};
        for _ in 0..8 {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u128(Instant::now().elapsed().as_nanos() ^ std::process::id() as u128);
            let dir = std::env::temp_dir().join(format!("pdfr-ai-{:016x}", h.finish()));
            #[cfg_attr(not(unix), allow(unused_mut))]
            let mut b = std::fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                b.mode(0o700);
            }
            // `create` (not `create_all`) fails if it exists: never reuse a
            // directory someone else made.
            if b.create(&dir).is_ok() {
                return Ok(TempDir(dir));
            }
        }
        Err("could not create a private temporary directory".into())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Stops the child and anything it started (its process group).
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        // SAFETY: plain syscall; the child was put in its own group at spawn.
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs a command with stdin input and a timeout. Returns stdout. The
/// timeout covers the whole exchange, including output still held open by
/// grandchildren; stderr is drained so a chatty tool cannot block.
fn run_with_timeout(mut cmd: Command, input: &str, timeout: Duration) -> Result<String, String> {
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let input = input.to_string();
    std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
    });
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stdout.read_to_string(&mut s);
        let _ = tx.send(s);
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let left = timeout
                    .saturating_sub(start.elapsed())
                    .max(Duration::from_secs(1));
                let Ok(out) = rx.recv_timeout(left) else {
                    kill_tree(&mut child);
                    return Err("timed out".into());
                };
                if status.success() {
                    return Ok(out);
                }
                return Err(format!("exit status {status}"));
            }
            Ok(None) if start.elapsed() > timeout => {
                kill_tree(&mut child);
                return Err("timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

impl Backend for CliBackend {
    fn name(&self) -> String {
        match self.model.as_str() {
            "" => format!("{} ({})", self.tool.name(), self.tool.binary()),
            m => format!("{} ({}, model {m})", self.tool.name(), self.tool.binary()),
        }
    }

    fn run(&self, instruction: &str, prompt: &str) -> Result<Reply, String> {
        // Run in an empty private directory so the tool sees no project.
        let tmp = TempDir::new()?;
        let dir = tmp.0.clone();
        match self.tool {
            Tool::Claude => {
                let mut c = Command::new(&self.path);
                c.current_dir(&dir).args([
                    "-p",
                    "--model",
                    &self.model,
                    "--effort",
                    "low",
                    "--output-format",
                    "json",
                    "--tools",
                    "",
                    "--no-session-persistence",
                    "--strict-mcp-config",
                    "--disable-slash-commands",
                    "--system-prompt",
                    instruction,
                ]);
                run_with_timeout(c, prompt, TIMEOUT).and_then(|out| parse_claude_json(&out))
            }
            Tool::Codex => {
                let mut c = Command::new(&self.path);
                c.current_dir(&dir).args([
                    "exec",
                    "--skip-git-repo-check",
                    "--ephemeral",
                    "--sandbox",
                    "read-only",
                    "--color",
                    "never",
                    "-m",
                    &self.model,
                    "--json",
                    "-",
                ]);
                let full = format!("{instruction}\n\n{prompt}");
                run_with_timeout(c, &full, TIMEOUT).and_then(|out| parse_codex_jsonl(&out))
            }
            Tool::Pi => {
                // No tools of any kind, nothing of the user's loaded, no
                // session kept.
                let mut c = Command::new(&self.path);
                c.current_dir(&dir).args([
                    "-p",
                    "--no-tools",
                    "--no-mcp",
                    "--no-extensions",
                    "--no-skills",
                    "--no-prompt-templates",
                    "--no-context-files",
                    "--no-session",
                    "--system-prompt",
                    instruction,
                ]);
                if !self.model.is_empty() {
                    c.args(["--model", &self.model]);
                }
                run_with_timeout(c, prompt, TIMEOUT).and_then(|out| plain_reply(&out))
            }
            Tool::MiniMax => {
                // "Lightweight" sends the model no tool at all, and no
                // skills, memory or workspace instructions.
                let mut c = Command::new(&self.path);
                c.current_dir(&dir).args([
                    "exec",
                    "--mode",
                    "lightweight",
                    "--output-format",
                    "json",
                    "--input",
                    "-",
                ]);
                if !self.model.is_empty() {
                    c.args(["--model", &self.model]);
                }
                let full = format!("{instruction}\n\n{prompt}");
                run_with_timeout(c, &full, TIMEOUT).and_then(|out| parse_minimax_json(&out))
            }
            Tool::Kimi => {
                // An agent of our own with an empty tool list (that covers
                // MCP tools too) and the instruction as its whole prompt;
                // skills are looked for in an empty folder.
                let agent = dir.join("agent.md");
                let skills = dir.join("skills");
                std::fs::create_dir(&skills).map_err(|e| e.to_string())?;
                std::fs::write(
                    &agent,
                    format!(
                        "---\nname: pdfr-labels\ndescription: Maps form-field labels to keys.\n\
                         tools: []\n---\n\n{instruction}\n"
                    ),
                )
                .map_err(|e| e.to_string())?;
                let mut c = Command::new(&self.path);
                c.current_dir(&dir)
                    .arg("--agent-file")
                    .arg(&agent)
                    .arg("--skills-dir")
                    .arg(&skills)
                    .args(["--output-format", "text"]);
                if !self.model.is_empty() {
                    c.args(["-m", &self.model]);
                }
                c.args(["-p", prompt]);
                run_with_timeout(c, "", TIMEOUT).and_then(|out| plain_reply(&out))
            }
            Tool::Ollama | Tool::LmStudio => Err("not a command-line tool".into()),
        }
    }
}

/// A reply printed as plain text. Kimi Code starts paragraphs with a bullet.
pub fn plain_reply(out: &str) -> Result<Reply, String> {
    let text = out
        .lines()
        .map(|l| l.trim_start().trim_start_matches('\u{2022}').trim())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("no reply".into());
    }
    Ok(Reply {
        text,
        ..Default::default()
    })
}

/// `mcode exec --output-format json`: one object with `output`, or `error`.
pub fn parse_minimax_json(out: &str) -> Result<Reply, String> {
    let v: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("bad JSON from mcode: {e}"))?;
    if let Some(e) = v.get("error").filter(|e| !e.is_null()) {
        return Err(e["message"]
            .as_str()
            .or(e.as_str())
            .unwrap_or("error")
            .to_string());
    }
    let text = v["output"].as_str().ok_or("no output")?.to_string();
    let u = &v["usage"];
    let g = |a: &str, b: &str| u[a].as_u64().or(u[b].as_u64());
    Ok(Reply {
        text,
        input_tokens: g("inputTokens", "input_tokens"),
        output_tokens: g("outputTokens", "output_tokens"),
    })
}

/// `claude -p --output-format json`: one JSON object with `result` and `usage`.
pub fn parse_claude_json(out: &str) -> Result<Reply, String> {
    let v: serde_json::Value =
        serde_json::from_str(out.trim()).map_err(|e| format!("bad JSON from claude: {e}"))?;
    if v.get("is_error").and_then(|b| b.as_bool()) == Some(true) {
        return Err(v
            .get("result")
            .and_then(|r| r.as_str())
            .unwrap_or("error")
            .to_string());
    }
    let text = v
        .get("result")
        .and_then(|r| r.as_str())
        .ok_or("no result")?
        .to_string();
    let u = v.get("usage");
    let g = |k: &str| u.and_then(|u| u.get(k)).and_then(|x| x.as_u64());
    let input = match (
        g("input_tokens"),
        g("cache_creation_input_tokens"),
        g("cache_read_input_tokens"),
    ) {
        (None, None, None) => None,
        (a, b, c) => Some(a.unwrap_or(0) + b.unwrap_or(0) + c.unwrap_or(0)),
    };
    Ok(Reply {
        text,
        input_tokens: input,
        output_tokens: g("output_tokens"),
    })
}

/// `codex exec --json`: JSONL events; the agent message and turn usage.
pub fn parse_codex_jsonl(out: &str) -> Result<Reply, String> {
    let mut r = Reply::default();
    for line in out.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(item) = v.get("item")
            && item.get("type").and_then(|t| t.as_str()) == Some("agent_message")
            && let Some(t) = item.get("text").and_then(|t| t.as_str())
        {
            r.text = t.to_string();
        }
        if let Some(u) = v.get("usage") {
            let g = |k: &str| u.get(k).and_then(|x| x.as_u64());
            r.input_tokens = Some(r.input_tokens.unwrap_or(0) + g("input_tokens").unwrap_or(0));
            r.output_tokens = Some(r.output_tokens.unwrap_or(0) + g("output_tokens").unwrap_or(0));
        }
    }
    if r.text.is_empty() {
        return Err("no reply from codex".into());
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model server for one test: answers each request with the canned
    /// body for its path, and gives back what it was sent.
    fn fake_server(
        replies: Vec<(&'static str, String)>,
    ) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut len = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&line);
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                let _ = tx.send(format!("{path} {}", String::from_utf8_lossy(&body)));
                let reply = replies
                    .iter()
                    .find(|r| r.0 == path)
                    .map_or("{}".to_string(), |r| r.1.clone());
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        (base, rx)
    }

    #[test]
    fn a_model_server_is_asked_in_the_openai_style() {
        let (base, sent) = fake_server(vec![
            (
                "/v1/models",
                r#"{"data":[{"id":"llama3.2:3b"},{"id":"other"}]}"#.into(),
            ),
            (
                "/v1/chat/completions",
                r#"{"choices":[{"message":{"content":"<think>hm</think>\n1:email"}}],
                    "usage":{"prompt_tokens":40,"completion_tokens":3}}"#
                    .into(),
            ),
        ]);
        // No model named: the first one the server has.
        let b = HttpBackend {
            tool: Tool::Ollama,
            base,
            model: String::new(),
        };
        assert_eq!(b.name(), "Ollama (on this computer)");
        let r = b.run("INSTRUCTION", "LABELS").unwrap();
        assert_eq!(r.text, "1:email");
        assert_eq!((r.input_tokens, r.output_tokens), (Some(40), Some(3)));
        assert!(sent.recv().unwrap().starts_with("/v1/models"));
        let chat = sent.recv().unwrap();
        let body: serde_json::Value =
            serde_json::from_str(chat.strip_prefix("/v1/chat/completions ").unwrap()).unwrap();
        assert_eq!(body["model"], "llama3.2:3b");
        assert_eq!(body["messages"][0]["content"], "INSTRUCTION");
        assert_eq!(body["messages"][1]["content"], "LABELS");
        // Nothing but the model and the two messages: no tools are offered.
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn a_server_error_is_told() {
        let e = parse_chat_json(r#"{"error":{"message":"model not found"}}"#).unwrap_err();
        assert_eq!(e, "model not found");
        assert!(parse_chat_json("{}").is_err());
    }

    #[test]
    fn replies_of_the_other_tools() {
        // Kimi Code prints paragraphs behind a bullet.
        let r = plain_reply("\u{2022} 1:email\n2:none\n").unwrap();
        assert_eq!(r.text, "1:email\n2:none");
        assert!(plain_reply(" \n").is_err());
        let r = parse_minimax_json(
            r#"{"schemaVersion":1,"type":"exec.result","output":"1:email","usage":{"inputTokens":9,"outputTokens":2}}"#,
        )
        .unwrap();
        assert_eq!(r.text, "1:email");
        assert_eq!((r.input_tokens, r.output_tokens), (Some(9), Some(2)));
        assert!(parse_minimax_json(r#"{"error":{"message":"no key"}}"#).is_err());
    }

    #[test]
    fn tools_keep_their_ids() {
        for t in TOOLS {
            assert_eq!(Tool::from_id(t.id()), Some(t));
        }
        // Codex cannot be run without its tools: never offered.
        assert_eq!(Tool::from_id("codex"), None);
        assert_eq!(Tool::from_id(""), None);
    }

    fn vocab() -> Vec<String> {
        ["full_name", "email", "date_of_birth"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn prompt_is_compact_and_numbered() {
        let p = build_prompt(&["Nombre del titular".into(), "x".repeat(200)], &vocab());
        assert!(
            p.starts_with("KEYS: full_name,email,date_of_birth\nLABELS:\n1:Nombre del titular\n2:")
        );
        assert!(p.lines().last().unwrap().len() <= 2 + MAX_LABEL_CHARS);
    }

    #[test]
    fn parses_lines_and_json() {
        assert_eq!(
            parse_reply("1:key:email\n2:none", 2, &vocab()).unwrap()[0],
            Some(Answer::Key("email".into()))
        );
        let r = parse_reply(
            "1:full_name\n2:none\n3:new:pet_name\n4:date_of_birth.day\n5:full_name|ff",
            5,
            &vocab(),
        )
        .unwrap();
        assert_eq!(r[0], Some(Answer::Key("full_name".into())));
        assert_eq!(r[1], Some(Answer::None));
        assert_eq!(r[2], Some(Answer::New("pet_name".into())));
        assert_eq!(r[3], Some(Answer::Key("date_of_birth.day".into())));
        assert_eq!(r[4], Some(Answer::Key("full_name|ff".into())));
        let j = parse_reply(r#"["email","none"]"#, 2, &vocab()).unwrap();
        assert_eq!(j[0], Some(Answer::Key("email".into())));
    }

    #[test]
    fn malformed_replies_are_rejected() {
        for bad in [
            "",
            "Sure! Here you go:",
            "1:unknown_key",
            "7:email",
            "1 email",
            "[\"email\"]",
            "1:new:Bad Key!",
            "1:email.week",
            "1:none",
            "1:email\n1:none\n2:none",
        ] {
            assert!(parse_reply(bad, 2, &vocab()).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn claude_and_codex_outputs() {
        let c = parse_claude_json(r#"{"type":"result","is_error":false,"result":"1:email","usage":{"input_tokens":120,"cache_read_input_tokens":30,"output_tokens":6}}"#).unwrap();
        assert_eq!(
            (c.text.as_str(), c.input_tokens, c.output_tokens),
            ("1:email", Some(150), Some(6))
        );
        let x = parse_codex_jsonl("{\"type\":\"thread.started\"}\n{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"1:none\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":200,\"cached_input_tokens\":0,\"output_tokens\":5}}\n").unwrap();
        assert_eq!(
            (x.text.as_str(), x.input_tokens, x.output_tokens),
            ("1:none", Some(200), Some(5))
        );
    }
}
