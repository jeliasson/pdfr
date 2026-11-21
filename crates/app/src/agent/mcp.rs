//! `pdfr mcp`: a Model Context Protocol server on stdin/stdout, so an AI
//! agent can open pdfr windows and drive them. JSON-RPC 2.0, one message
//! per line. It shows no window itself: each tool call is passed to a pdfr
//! window over its socket (see [`super::bridge`]).

use super::bridge::{self, Client, Failure};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Protocol versions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// How long a window gets to answer. Its own limits are shorter, except
/// when it is waiting for the user to allow something.
const CALL_WAIT: Duration = Duration::from_secs(170);

/// How long a new window gets to appear.
const START_WAIT: Duration = Duration::from_secs(20);

const INSTRUCTIONS: &str = "\
pdfr is a desktop app for viewing PDFs and filling in forms. These tools drive a real pdfr \
window on the user's screen; the user watches and can take over at any time.

Typical use: `open` a PDF, `read_form` to list its boxes (each has a `ref`), `fill` / \
`set_checked` / `choose` by ref, `check_form`, then `save_as`. This is the fast, reliable way \
to fill a form, and works for real form fields, flat forms and scans alike.

To see the window, call `screenshot`. To act like a person, use `computer` (click, type, \
key, scroll, drag): its x and y are pixels of the latest screenshot. `read_form` also gives \
each visible box's position in screenshot pixels (`at`).

Pages are numbered from 1. Every answer ends with the window's `state` (file, page, unsaved \
changes, any dialog that is open). Errors carry a code: `paused_by_user` (the user took over; \
stop and ask them), `needs_confirmation`, `declined` (the user said no; do not retry), \
`no_window`, `busy`.";

/// Runs the server until stdin closes. Returns the process exit code.
pub fn run(args: Vec<std::ffi::OsString>) -> i32 {
    let mut data_dir = None;
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        match a.to_string_lossy().as_ref() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!(
                    "pdfr mcp — let an AI agent control pdfr (Model Context Protocol on \
                     stdin/stdout)\n\nAdd it to your agent, for example:\n  claude mcp add pdfr -- pdfr mcp"
                );
                return 0;
            }
            other => {
                eprintln!("pdfr mcp: unknown option {other}");
                return 2;
            }
        }
    }
    let mut server = Mcp::new(data_dir);
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        if let Some(a) = answer
            && (writeln!(out, "{a}").is_err() || out.flush().is_err())
        {
            break;
        }
    }
    0
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub struct Mcp {
    /// Passed on to windows this server starts (tests).
    data_dir: Option<PathBuf>,
    /// The window the last call went to.
    current: Option<(PathBuf, Client)>,
}

impl Mcp {
    pub fn new(data_dir: Option<PathBuf>) -> Mcp {
        Mcp {
            data_dir,
            current: None,
        }
    }

    /// Answers one message; `None` for notifications.
    pub fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg["method"].as_str().unwrap_or_default();
        let Some(id) = msg.get("id").filter(|i| !i.is_null()).cloned() else {
            return None; // a notification: nothing to answer
        };
        let result = match method {
            "initialize" => {
                let asked = msg["params"]["protocolVersion"]
                    .as_str()
                    .unwrap_or_default();
                let version = VERSIONS
                    .iter()
                    .find(|v| **v == asked)
                    .unwrap_or(&VERSIONS[0]);
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {
                        "name": "pdfr",
                        "title": "pdfr",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "instructions": INSTRUCTIONS,
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools()}),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or_default();
                let empty = json!({});
                let args = msg["params"].get("arguments").unwrap_or(&empty);
                if !tools().iter().any(|t| t["name"] == name) {
                    return Some(rpc_error(id, -32602, &format!("Unknown tool: {name}")));
                }
                self.call_tool(name, args)
            }
            _ => {
                return Some(rpc_error(
                    id,
                    -32601,
                    &format!("Method not found: {method}"),
                ));
            }
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    /// Runs a tool and wraps what it gives as MCP content.
    fn call_tool(&mut self, name: &str, args: &Value) -> Value {
        match self.run_tool(name, args) {
            Ok((mut result, state)) => {
                let mut content = vec![];
                // A screenshot travels as an image, not as text.
                if let Some(data) = result.get("image").and_then(|d| d.as_str()) {
                    content.push(json!({"type": "image", "data": data, "mimeType": "image/png"}));
                    result.as_object_mut().map(|o| o.remove("image"));
                }
                if !state.is_null()
                    && let Some(o) = result.as_object_mut()
                {
                    o.insert("state".into(), state);
                }
                content.push(json!({"type": "text", "text": result.to_string()}));
                json!({"content": content, "isError": false})
            }
            Err(f) => json!({
                "content": [{"type": "text", "text": format!("{}: {}", f.code, f.message)}],
                "isError": true,
            }),
        }
    }

    fn run_tool(&mut self, name: &str, args: &Value) -> Result<(Value, Value), Failure> {
        match name {
            "windows_list" => Ok((json!({"windows": self.windows()}), Value::Null)),
            "open" => self.open(args),
            _ => {
                let pid = args["window"].as_u64().map(|p| p as u32);
                self.window(pid)?.call(name, args, CALL_WAIT)
            }
        }
    }

    /// Every running window, with what it has open.
    fn windows(&mut self) -> Vec<Value> {
        bridge::sockets()
            .iter()
            .filter_map(|s| {
                let mut c = bridge::connect_or_clean(s)?;
                let (_, state) = c.call("state", &json!({}), Duration::from_secs(5)).ok()?;
                Some(json!({"window": bridge::pid_of(s), "state": state}))
            })
            .collect()
    }

    /// The window a call goes to: the one asked for, else the one used
    /// last, else the newest one running.
    fn window(&mut self, pid: Option<u32>) -> Result<&mut Client, Failure> {
        let wanted = |s: &PathBuf| pid.is_none_or(|p| bridge::pid_of(s) == Some(p));
        let reuse = self
            .current
            .as_ref()
            .is_some_and(|(s, _)| wanted(s) && s.exists());
        if !reuse {
            self.current = bridge::sockets()
                .into_iter()
                .rev()
                .filter(|s| wanted(s))
                .find_map(|s| bridge::connect_or_clean(&s).map(|c| (s, c)));
        }
        match (&mut self.current, pid) {
            (Some((_, c)), _) => Ok(c),
            (None, Some(p)) => Err(Failure::new(
                "no_window",
                format!("There is no pdfr window {p}. Use windows_list to see the open ones."),
            )),
            (None, None) => Err(Failure::new(
                "no_window",
                "No pdfr window is open (or agent control is turned off in pdfr's Settings). \
                 Use open to start one.",
            )),
        }
    }

    /// `open`: in the current window, or in a new one when there is none
    /// (or one is asked for).
    fn open(&mut self, args: &Value) -> Result<(Value, Value), Failure> {
        let new_window = args["new_window"].as_bool().unwrap_or(false);
        let pid = args["window"].as_u64().map(|p| p as u32);
        if !new_window && self.window(pid).is_ok() {
            return self.window(pid)?.call("open", args, CALL_WAIT);
        }
        let before = bridge::sockets();
        let exe = std::env::current_exe()
            .map_err(|e| Failure::new("failed", format!("Could not find pdfr: {e}.")))?;
        let mut cmd = std::process::Command::new(exe);
        cmd.arg("--agent");
        if let Some(d) = &self.data_dir {
            cmd.arg("--data-dir").arg(d);
        }
        // The window must not write into this server's stdout.
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let mut child = cmd
            .spawn()
            .map_err(|e| Failure::new("failed", format!("Could not start pdfr: {e}.")))?;
        let started = Instant::now();
        let found = loop {
            let new = bridge::sockets()
                .into_iter()
                .filter(|s| !before.contains(s))
                .find_map(|s| bridge::connect_or_clean(&s).map(|c| (s, c)));
            if new.is_some() {
                break new;
            }
            // The window closed again (no display, or it crashed).
            if child.try_wait().ok().flatten().is_some() || started.elapsed() > START_WAIT {
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        // The window lives on by itself; do not leave a zombie behind.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        self.current = Some(found.ok_or_else(|| {
            Failure::new(
                "no_window",
                "pdfr started but did not accept agents. Is \"Allow AI agents\" turned on in \
                 pdfr's Settings, and is there a display to show a window on?",
            )
        })?);
        let c = &mut self.current.as_mut().unwrap().1;
        if args["path"].is_string() {
            c.call("open", args, CALL_WAIT)
        } else {
            c.call("state", &json!({}), CALL_WAIT)
        }
    }
}

/// One tool: every tool also takes `window`.
fn tool(name: &str, description: &str, mut props: Value, required: &[&str]) -> Value {
    props["window"] = json!({
        "type": "integer",
        "description": "Which pdfr window (from windows_list). Leave out for the one used last.",
    });
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": props, "required": required},
    })
}

/// The tools an agent sees.
pub fn tools() -> Vec<Value> {
    let field_ref =
        json!({"type": "string", "description": "A box's ref from read_form, like \"f12\"."});
    vec![
        tool(
            "windows_list",
            "List the open pdfr windows and what each has open.",
            json!({}),
            &[],
        ),
        tool(
            "open",
            "Open a PDF in pdfr. Starts pdfr if it is not running. Waits until the form's boxes \
             have been found, then returns the window's state.",
            json!({
                "path": {"type": "string", "description": "Absolute path of the PDF. Leave out to just start pdfr."},
                "password": {"type": "string", "description": "For a password-protected PDF."},
                "new_window": {"type": "boolean", "description": "Open in a new window instead of the current one."},
                "discard": {"type": "boolean", "description": "Replace the open document even if it has unsaved changes."},
            }),
            &[],
        ),
        tool(
            "close",
            "Close the pdfr window.",
            json!({"discard": {"type": "boolean", "description": "Close even if there are unsaved changes."}}),
            &[],
        ),
        tool(
            "screenshot",
            "A picture of the pdfr window as the user sees it. Positions for `computer` are pixels of this picture.",
            json!({}),
            &[],
        ),
        tool(
            "read_form",
            "List the form's boxes in reading order: ref, page, kind (text, line, multiline, \
             boxes = one character per square, checkbox, radio, choice, signature), label, \
             value, and where visible, `at` = its centre in screenshot pixels.",
            json!({
                "page": {"type": "integer", "description": "Only this page (from 1)."},
                "only_empty": {"type": "boolean", "description": "Only boxes not filled in yet."},
                "filter": {"type": "string", "description": "Only boxes whose label or name contains this text."},
            }),
            &[],
        ),
        tool(
            "fill",
            "Type a value into a text box (replacing what is there). Rows of squares take one \
             character each and long values carry on into the next row. One undo step.",
            json!({"ref": field_ref, "value": {"type": "string"}}),
            &["ref", "value"],
        ),
        tool(
            "set_checked",
            "Tick or clear a checkbox, or select a radio button.",
            json!({"ref": field_ref, "checked": {"type": "boolean"}}),
            &["ref", "checked"],
        ),
        tool(
            "choose",
            "Pick an option of a drop-down (by its text or its value).",
            json!({"ref": field_ref, "option": {"type": "string"}}),
            &["ref", "option"],
        ),
        tool(
            "suggest",
            "What pdfr remembers the user usually writes in this box (names, addresses, …), \
             best first. Empty if nothing is remembered or the user turned this off.",
            json!({"ref": field_ref}),
            &["ref"],
        ),
        tool(
            "read_text",
            "The text printed on a page, line by line.",
            json!({"page": {"type": "integer", "description": "Page number, from 1."}}),
            &["page"],
        ),
        tool(
            "find",
            "Search the document's text. Gives each match's page and the words around it.",
            json!({"query": {"type": "string"}}),
            &["query"],
        ),
        tool(
            "check_form",
            "Answers that look wrong (a date that cannot be, a bad IBAN, …) and boxes still empty.",
            json!({}),
            &[],
        ),
        tool(
            "view",
            "Scroll to a page or a box, or change the zoom.",
            json!({
                "page": {"type": "integer", "description": "Go to this page (from 1)."},
                "ref": {"type": "string", "description": "Bring this box into view and put the caret in it."},
                "zoom": {"type": "number", "description": "1.0 is actual size."},
            }),
            &[],
        ),
        tool(
            "run_command",
            "Run one of pdfr's commands by name (as in its command palette), e.g. Undo, Redo, \
             FitWidth, NextEmpty, Sidebar. With no name: lists the commands that can run now.",
            json!({"name": {"type": "string"}}),
            &[],
        ),
        tool(
            "save",
            "Save to the file this document was last saved as. A document not saved yet needs save_as.",
            json!({}),
            &[],
        ),
        tool(
            "save_as",
            "Save the filled-in PDF to a path. The original is never changed unless you save over it.",
            json!({
                "path": {"type": "string", "description": "Absolute path of the PDF to write."},
                "overwrite": {"type": "boolean", "description": "Replace a file that is already there. The user may be asked to allow it."},
            }),
            &["path"],
        ),
        tool(
            "print",
            "Print the document. Opens the print dialog for the user to finish.",
            json!({}),
            &[],
        ),
        tool(
            "wait_idle",
            "Wait until pdfr has finished loading: pages read and boxes found.",
            json!({"timeout_ms": {"type": "integer"}}),
            &[],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(m: &mut Mcp, msg: Value) -> Option<Value> {
        m.handle(&msg)
    }

    #[test]
    fn initialize_echoes_a_known_version() {
        let mut m = Mcp::new(None);
        let a = ask(
            &mut m,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}),
        )
        .unwrap();
        assert_eq!(a["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(a["result"]["serverInfo"]["name"], "pdfr");
        assert!(a["result"]["capabilities"]["tools"].is_object());
        // An unknown version gets this server's newest.
        let a = ask(
            &mut m,
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        )
        .unwrap();
        assert_eq!(a["result"]["protocolVersion"], VERSIONS[0]);
    }

    #[test]
    fn notifications_get_no_answer() {
        let mut m = Mcp::new(None);
        assert!(
            ask(
                &mut m,
                json!({"jsonrpc":"2.0","method":"notifications/initialized"})
            )
            .is_none()
        );
    }

    #[test]
    fn tools_are_listed_with_schemas() {
        let mut m = Mcp::new(None);
        let a = ask(
            &mut m,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        )
        .unwrap();
        let tools = a["result"]["tools"].as_array().unwrap();
        for name in ["open", "read_form", "fill", "screenshot", "save_as"] {
            let t = tools.iter().find(|t| t["name"] == name).expect(name);
            assert_eq!(t["inputSchema"]["type"], "object");
            assert!(t["inputSchema"]["properties"]["window"].is_object());
        }
        let fill = tools.iter().find(|t| t["name"] == "fill").unwrap();
        assert_eq!(fill["inputSchema"]["required"], json!(["ref", "value"]));
    }

    #[test]
    fn unknown_methods_and_tools_are_errors() {
        let mut m = Mcp::new(None);
        let a = ask(&mut m, json!({"jsonrpc":"2.0","id":4,"method":"nope"})).unwrap();
        assert_eq!(a["error"]["code"], -32601);
        let a = ask(
            &mut m,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}),
        )
        .unwrap();
        assert_eq!(a["error"]["code"], -32602);
        let a = ask(&mut m, json!({"jsonrpc":"2.0","id":6,"method":"ping"})).unwrap();
        assert_eq!(a["result"], json!({}));
    }
}
