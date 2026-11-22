//! Pointer and keyboard input from an AI agent: the `computer` tool. The
//! agent's clicks, keys and text are fed to the window as input events,
//! one step per drawn frame, so everything reacts exactly as it does to a
//! person; a pointer is drawn where the agent's is. The demo recorder
//! (`record.rs`) works the same way from a fixed script.
//!
//! Positions come in pixels of the screenshot the agent was sent.

use super::agent::{Outcome, Wait, fail};
use super::*;
use serde_json::Value;
use std::collections::VecDeque;
use std::time::Duration;

/// How long input may take before the agent is told the window is stuck.
const INPUT_WAIT: Duration = Duration::from_secs(12);
/// Frames drawn after the last step before the agent is answered.
const SETTLE_PASSES: u64 = 3;
/// Characters typed per frame.
const TYPE_CHUNK: usize = 12;
/// How long the agent's pointer stays drawn after it last moved.
const POINTER_SHOWN: Duration = Duration::from_secs(4);
/// A request this recent makes the user's own click or key a takeover.
const AT_WORK: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
enum Step {
    /// Glide the pointer here.
    Move(Pos2),
    Button(egui::PointerButton, bool, Modifiers),
    Text(String),
    Key(Key, Modifiers),
    /// Scroll by this much (points; positive moves the content down/right).
    Wheel(Vec2),
    /// The modifier keys held from here on (Shift+click).
    Held(Modifiers),
}

#[derive(Default)]
pub(super) struct Input {
    steps: VecDeque<Step>,
    /// Where the agent's pointer is (window points).
    pointer: Option<Pos2>,
    /// The step under way: when it started and where the pointer was then.
    moving: Option<(Instant, Pos2)>,
    /// The frame the last step was fed before (one step per frame).
    fed_pass: Option<u64>,
    /// The frame at which the last step was fed.
    done_pass: Option<u64>,
    last_action: Option<Instant>,
}

impl Input {
    pub(super) fn clear(&mut self) {
        self.steps.clear();
        self.moving = None;
    }
}

/// A person's deliberate input (not the pointer merely passing over).
fn user_acted(e: &Event) -> bool {
    matches!(
        e,
        Event::PointerButton { pressed: true, .. }
            | Event::Key { pressed: true, .. }
            | Event::Text(_)
            | Event::Paste(_)
            | Event::MouseWheel { .. }
    )
}

/// Ctrl on Linux and Windows, as pdfr's shortcuts are written; on a Mac
/// the same shortcuts use Cmd.
fn ctrl() -> Modifiers {
    if cfg!(target_os = "macos") {
        Modifiers::MAC_CMD | Modifiers::COMMAND
    } else {
        Modifiers::CTRL | Modifiers::COMMAND
    }
}

/// `"ctrl+shift+s"` → the key and its modifiers.
fn parse_chord(chord: &str) -> Result<(Key, Modifiers), String> {
    let mut mods = Modifiers::NONE;
    let mut key = None;
    for part in chord.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" | "cmd" | "command" | "meta" | "super" => mods |= ctrl(),
            "shift" => mods |= Modifiers::SHIFT,
            "alt" | "option" => mods |= Modifiers::ALT,
            name => {
                let alias = match name {
                    "esc" => "Escape",
                    "return" => "Enter",
                    "del" => "Delete",
                    "pgup" => "PageUp",
                    "pgdn" | "pgdown" => "PageDown",
                    "up" => "ArrowUp",
                    "down" => "ArrowDown",
                    "left" => "ArrowLeft",
                    "right" => "ArrowRight",
                    _ => "",
                };
                // As written, as an alias, or with a capital ("tab" → "Tab").
                let capital: String = part
                    .chars()
                    .enumerate()
                    .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
                    .collect();
                key = Key::from_name(part)
                    .or_else(|| Key::from_name(alias))
                    .or_else(|| Key::from_name(&capital))
                    .or_else(|| Key::from_name(&part.to_uppercase()));
                if key.is_none() {
                    return Err(format!("\"{part}\" is not a key pdfr knows."));
                }
            }
        }
    }
    key.map(|k| (k, mods))
        .ok_or_else(|| format!("\"{chord}\" names no key."))
}

/// The text a real keyboard sends along with this key press, if any.
fn typed_with(key: Key, mods: Modifiers) -> Option<String> {
    if mods.ctrl || mods.command || mods.alt || mods.mac_cmd {
        return None;
    }
    if key == Key::Space {
        return Some(" ".into());
    }
    let name = key.name();
    let mut cs = name.chars();
    match (cs.next(), cs.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(if mods.shift {
            c.to_ascii_uppercase().to_string()
        } else {
            c.to_ascii_lowercase().to_string()
        }),
        _ => None,
    }
}

impl App {
    /// Why an agent's clicks and keys are not let through now, if they are
    /// not: Settings, the question whether AI may be used and the licence
    /// dialog are the user's alone.
    pub(super) fn agent_kept_out(&self) -> Option<&'static str> {
        if self.assist.settings_open {
            Some(
                "pdfr's Settings are open. They are the user's to change; ask them to close Settings.",
            )
        } else if self.assist.consent_open() {
            Some("pdfr is asking the user whether it may use AI. Only they can answer.")
        } else if self.license.modal_open() {
            Some("pdfr's licence dialog is open. It is the user's; ask them to close it.")
        } else {
            None
        }
    }

    /// `computer`: queues the steps of one action; the request is answered
    /// when they have all taken effect.
    pub(super) fn agent_computer(&mut self, args: &Value, ctx: &egui::Context) -> Outcome {
        if self.agent.ask.is_some() {
            return fail(
                "busy",
                "pdfr is waiting for the user to answer a question; it cannot be clicked away.",
            );
        }
        if let Some(why) = self.agent_kept_out() {
            return fail("busy", why);
        }
        if !self.viewport.is_positive() {
            return fail(
                "busy",
                "The window has not been drawn yet. Call wait_idle first.",
            );
        }
        // Screenshot pixels → window points.
        let px = self.agent_px(ctx);
        let point = |v: &Value, w: &Value| -> Option<Pos2> {
            Some(pos2(v.as_f64()? as f32 / px, w.as_f64()? as f32 / px))
        };
        let at = point(&args["x"], &args["y"]);
        let need_at = || {
            at.ok_or_else(|| "x and y (pixels of the latest screenshot) are needed.".to_string())
        };
        let mods = match args["modifiers"].as_str() {
            Some(m) => {
                let mut out = Modifiers::NONE;
                for part in m.split('+').map(|p| p.trim().to_lowercase()) {
                    match part.as_str() {
                        "" => {}
                        "shift" => out |= Modifiers::SHIFT,
                        "alt" | "option" => out |= Modifiers::ALT,
                        "ctrl" | "control" | "cmd" | "command" => out |= ctrl(),
                        other => return fail("bad_request", format!("Unknown modifier {other}.")),
                    }
                }
                out
            }
            None => Modifiers::NONE,
        };
        let button = match args["button"].as_str().unwrap_or("left") {
            "left" => egui::PointerButton::Primary,
            "right" => egui::PointerButton::Secondary,
            "middle" => egui::PointerButton::Middle,
            other => return fail("bad_request", format!("Unknown button {other}.")),
        };
        let action = args["action"].as_str().unwrap_or_default();
        let steps: Result<Vec<Step>, String> = (|| {
            Ok(match action {
                "move" => vec![Step::Move(need_at()?)],
                "click" | "double_click" => {
                    let mut s = vec![Step::Move(need_at()?), Step::Held(mods)];
                    for _ in 0..if action == "click" { 1 } else { 2 } {
                        s.push(Step::Button(button, true, mods));
                        s.push(Step::Button(button, false, mods));
                    }
                    s.push(Step::Held(Modifiers::NONE));
                    s
                }
                "drag" => {
                    let to = args["to"]
                        .as_array()
                        .and_then(|a| point(a.first()?, a.get(1)?))
                        .ok_or("to: [x, y] says where the drag ends.")?;
                    vec![
                        Step::Move(need_at()?),
                        Step::Held(mods),
                        Step::Button(button, true, mods),
                        Step::Move(to),
                        Step::Button(button, false, mods),
                        Step::Held(Modifiers::NONE),
                    ]
                }
                "scroll" => {
                    let d = vec2(
                        args["dx"].as_f64().unwrap_or(0.0) as f32,
                        args["dy"].as_f64().unwrap_or(0.0) as f32,
                    ) / px;
                    if d == Vec2::ZERO {
                        return Err("dy (and/or dx) says how far to scroll, in pixels; \
                                    positive dy scrolls down."
                            .into());
                    }
                    // Over the page unless a place is given.
                    let over = at.unwrap_or(self.viewport.center());
                    vec![Step::Move(over), Step::Wheel(-d)]
                }
                "type" => {
                    let text = args["text"].as_str().ok_or("text is what to type.")?;
                    let chars: Vec<char> = text.chars().collect();
                    chars
                        .chunks(TYPE_CHUNK)
                        .map(|c| Step::Text(c.iter().collect()))
                        .collect()
                }
                "key" => {
                    let keys = args["keys"]
                        .as_str()
                        .ok_or("keys names the key, like \"Enter\" or \"ctrl+s\".")?;
                    let mut s = vec![];
                    for chord in keys.split_whitespace() {
                        let (k, m) = parse_chord(chord)?;
                        s.push(Step::Key(k, m));
                    }
                    s
                }
                other => {
                    return Err(format!(
                        "Unknown action \"{other}\": use click, double_click, move, drag, \
                         scroll, type or key."
                    ));
                }
            })
        })();
        match steps {
            Ok(steps) if steps.is_empty() => fail("bad_request", "Nothing to do."),
            Ok(steps) => {
                let inp = &mut self.agent.input;
                // Nothing is held for a plain click.
                let plain = mods == Modifiers::NONE;
                inp.steps.extend(
                    steps
                        .into_iter()
                        .filter(|s| !(plain && matches!(s, Step::Held(_)))),
                );
                inp.done_pass = None;
                ctx.request_repaint();
                Outcome::Wait(Wait::Input, INPUT_WAIT)
            }
            Err(e) => fail("bad_request", e),
        }
    }

    /// All queued input has been fed and has had frames to take effect.
    pub(super) fn agent_input_done(&self, ctx: &egui::Context) -> bool {
        let inp = &self.agent.input;
        inp.steps.is_empty()
            && inp
                .done_pass
                .is_none_or(|p| ctx.cumulative_pass_nr() >= p + SETTLE_PASSES)
            // A wheel turn is played out over a few frames.
            && ctx.input(|i| i.smooth_scroll_delta == Vec2::ZERO)
    }

    /// Feeds the next step of the agent's input into this frame's events
    /// (called just before each frame). Also notices the user taking over.
    pub(super) fn agent_input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Input that no frame has consumed yet (a hidden window ticks
        // without drawing) comes by again: neither feed nor judge it twice.
        let pass = ctx.cumulative_pass_nr();
        if self.agent.input.fed_pass == Some(pass) {
            return;
        }
        self.agent.input.fed_pass = Some(pass);

        // The user clicking or typing while an agent is at work takes over
        // (answering a question the agent raised is not taking over).
        let at_work = !self.agent.input.steps.is_empty()
            || self
                .agent
                .last_request
                .is_some_and(|t| t.elapsed() < AT_WORK);
        if at_work
            && !self.agent.paused
            && self.agent.ask.is_none()
            && raw.events.iter().any(user_acted)
        {
            self.agent_pause();
        }
        // Nothing of the agent's gets through while the user is asked
        // something, or has paused it.
        if self.agent.paused || self.agent.ask.is_some() {
            return;
        }
        // Nor into Settings (the switches that say what agents and AI may
        // do, what pdfr remembers), the AI question or the licence.
        if self.agent_kept_out().is_some() {
            self.agent.input.clear();
            return;
        }
        let inp = &mut self.agent.input;
        let Some(step) = inp.steps.front().cloned() else {
            return;
        };
        let now = Instant::now();
        let here = inp
            .pointer
            .or_else(|| ctx.pointer_latest_pos())
            .unwrap_or(ctx.viewport_rect().center());
        let done = match step {
            Step::Move(target) => {
                let (started, from) = *inp.moving.get_or_insert((now, here));
                // Quick, but slow enough for a person to follow.
                let secs = (from.distance(target) / 2500.0).clamp(0.08, 0.3);
                let k = (now.duration_since(started).as_secs_f32() / secs).min(1.0);
                let eased = k * k * (3.0 - 2.0 * k);
                let p = from + (target - from) * eased;
                inp.pointer = Some(p);
                raw.events.push(Event::PointerMoved(p));
                k >= 1.0
            }
            Step::Held(modifiers) => {
                raw.events.push(Event::ModifiersChanged(modifiers));
                true
            }
            Step::Button(button, pressed, modifiers) => {
                raw.events.push(Event::PointerButton {
                    pos: here,
                    button,
                    pressed,
                    modifiers,
                });
                inp.pointer = Some(here);
                true
            }
            Step::Text(t) => {
                raw.events.push(Event::Text(t));
                true
            }
            Step::Key(key, modifiers) => {
                for pressed in [true, false] {
                    raw.events.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers,
                    });
                    if pressed && let Some(t) = typed_with(key, modifiers) {
                        raw.events.push(Event::Text(t));
                    }
                }
                true
            }
            Step::Wheel(delta) => {
                raw.events.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: Modifiers::NONE,
                });
                true
            }
        };
        inp.last_action = Some(now);
        if done {
            inp.steps.pop_front();
            inp.moving = None;
            inp.done_pass = Some(pass);
        }
        ctx.request_repaint();
    }

    /// Draws the agent's pointer while it is at work with it.
    pub(super) fn agent_pointer(&mut self, ctx: &egui::Context) {
        let inp = &self.agent.input;
        let (Some(p), Some(at)) = (inp.pointer, inp.last_action) else {
            return;
        };
        if !inp.steps.is_empty() || at.elapsed() < POINTER_SHOWN {
            super::record::draw_pointer(ctx, p);
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(parse_chord("Enter").unwrap(), (Key::Enter, Modifiers::NONE));
        assert_eq!(parse_chord("tab").unwrap(), (Key::Tab, Modifiers::NONE));
        assert_eq!(parse_chord("esc").unwrap().0, Key::Escape);
        assert_eq!(parse_chord("down").unwrap().0, Key::ArrowDown);
        let (k, m) = parse_chord("ctrl+shift+s").unwrap();
        assert_eq!(k, Key::S);
        assert!(m.command && m.shift && !m.alt);
        assert_eq!(parse_chord("F1").unwrap().0, Key::F1);
        assert!(parse_chord("ctrl+").is_err());
        assert!(parse_chord("frobnicate").is_err());
    }

    #[test]
    fn keys_that_also_type() {
        assert_eq!(
            typed_with(Key::Space, Modifiers::NONE).as_deref(),
            Some(" ")
        );
        assert_eq!(typed_with(Key::A, Modifiers::NONE).as_deref(), Some("a"));
        assert_eq!(typed_with(Key::A, Modifiers::SHIFT).as_deref(), Some("A"));
        assert_eq!(typed_with(Key::S, ctrl()), None);
        assert_eq!(typed_with(Key::Enter, Modifiers::NONE), None);
    }
}
