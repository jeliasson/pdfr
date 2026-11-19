//! The XDG desktop portals pdfr talks to besides printing: which portals
//! exist (file dialogs need one), the desktop's colour scheme, and opening
//! links. One shared D-Bus session connection, made on first use.

use futures_lite::StreamExt;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, message::Type as MsgType};

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

fn connection() -> Option<Connection> {
    static CONN: OnceLock<Option<Connection>> = OnceLock::new();
    CONN.get_or_init(|| pollster::block_on(Connection::session()).ok())
        .clone()
}

/// Whether a portal interface (for example "FileChooser") is available.
/// Answers are cached; the first call talks to D-Bus (a few milliseconds).
pub fn has(iface: &str) -> bool {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(v) = cache.lock().unwrap().get(iface) {
        return *v;
    }
    let ok = connection().is_some_and(|c| {
        let full = format!("org.freedesktop.portal.{iface}");
        pollster::block_on(c.call_method(
            Some(DEST),
            PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(full.as_str(), "version"),
        ))
        .is_ok()
    });
    cache.lock().unwrap().insert(iface.to_string(), ok);
    ok
}

/// 0 unknown, 1 dark, 2 light, 3 no preference.
static SCHEME: AtomicU8 = AtomicU8::new(0);

pub fn prefers_dark() -> Option<bool> {
    match SCHEME.load(Ordering::Relaxed) {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn store_scheme(v: u32) {
    SCHEME.store(
        match v {
            1 => 1,
            2 => 2,
            _ => 3,
        },
        Ordering::Relaxed,
    );
}

/// `org.freedesktop.appearance color-scheme`: 0 no preference, 1 dark,
/// 2 light. Older portals wrap the value in one more variant.
fn unwrap_u32(v: &Value<'_>) -> Option<u32> {
    match v {
        Value::U32(n) => Some(*n),
        Value::Value(inner) => unwrap_u32(inner),
        _ => None,
    }
}

async fn read_scheme(c: &Connection) -> Option<u32> {
    for method in ["ReadOne", "Read"] {
        if let Ok(reply) = c
            .call_method(
                Some(DEST),
                PATH,
                Some("org.freedesktop.portal.Settings"),
                method,
                &("org.freedesktop.appearance", "color-scheme"),
            )
            .await
            && let Ok(v) = reply.body().deserialize::<OwnedValue>()
        {
            return unwrap_u32(&v);
        }
    }
    None
}

/// Reads the colour scheme now, then follows changes on a background
/// thread, repainting when it changes.
pub fn watch_color_scheme(ctx: eframe::egui::Context) {
    std::thread::Builder::new()
        .name("pdfr-colour-scheme".into())
        .spawn(move || {
            let Some(c) = connection() else { return };
            pollster::block_on(async {
                let Ok(rule) = MatchRule::builder()
                    .msg_type(MsgType::Signal)
                    .interface("org.freedesktop.portal.Settings")
                    .and_then(|b| b.member("SettingChanged"))
                    .map(|b| b.build())
                else {
                    return;
                };
                let stream = MessageStream::for_match_rule(rule, &c, None).await;
                if let Some(v) = read_scheme(&c).await {
                    store_scheme(v);
                    ctx.request_repaint();
                }
                let Ok(mut stream) = stream else { return };
                while let Some(Ok(msg)) = stream.next().await {
                    if let Ok((ns, key, v)) =
                        msg.body().deserialize::<(String, String, OwnedValue)>()
                        && ns == "org.freedesktop.appearance"
                        && key == "color-scheme"
                        && let Some(n) = unwrap_u32(&v)
                    {
                        store_scheme(n);
                        ctx.request_repaint();
                    }
                }
            });
        })
        .ok();
}

/// Opens a link with the user's browser through the OpenURI portal.
pub fn open_uri(uri: &str) -> Result<(), String> {
    let c = connection().ok_or("no desktop session bus")?;
    let opts: HashMap<&str, Value> = HashMap::new();
    pollster::block_on(c.call_method(
        Some(DEST),
        PATH,
        Some("org.freedesktop.portal.OpenURI"),
        "OpenURI",
        &("", uri, opts),
    ))
    .map(|_| ())
    .map_err(|_| "no portal to open links with".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_scheme_values() {
        assert_eq!(unwrap_u32(&Value::U32(1)), Some(1));
        assert_eq!(unwrap_u32(&Value::Value(Box::new(Value::U32(2)))), Some(2));
        assert_eq!(unwrap_u32(&Value::from("x")), None);
    }
}
