//! Layer 3 storage: what the user normally types, the permanent label cache,
//! settings and AI token totals. One JSON file in the platform's user data
//! directory, readable only by the user.

use crate::meaning::Meaning;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ValueEntry {
    pub value: String,
    pub count: u32,
    pub last_used: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    #[serde(default = "yes")]
    pub ai_enabled: bool,
    /// `None`: not asked yet.
    #[serde(default)]
    pub ai_consent: Option<bool>,
    /// Model name override; empty means the tool's cheapest model.
    #[serde(default)]
    pub model: String,
    /// Old "pause learning" switch; read as `remember = Never`.
    #[serde(default, skip_serializing)]
    pub learning_paused: bool,
    /// What happens to typed values when a document is saved.
    #[serde(default)]
    pub remember: Remember,
    /// Whether "open PDFs with pdfr by default?" was asked (once).
    #[serde(default)]
    pub default_app_asked: bool,
    /// Look for new versions once a day. `None`: not asked yet.
    #[serde(default)]
    pub update_check: Option<bool>,
    /// When versions were last looked up (Unix seconds).
    #[serde(default)]
    pub update_checked_at: u64,
    /// The newest version seen ("1.2.0"), empty if none.
    #[serde(default)]
    pub latest_version: String,
    /// A newer version the user dismissed; not shown again.
    #[serde(default)]
    pub dismissed_version: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Remember {
    /// Show what would be remembered and let the user choose.
    #[default]
    Ask,
    Always,
    Never,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            ai_enabled: true,
            ai_consent: None,
            model: String::new(),
            learning_paused: false,
            remember: Remember::Ask,
            default_app_asked: false,
            update_check: None,
            update_checked_at: 0,
            latest_version: String::new(),
            dismissed_version: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Per call: (labels sent, input tokens, output tokens).
    #[serde(default)]
    pub log: Vec<(usize, u64, u64)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Store {
    #[serde(default)]
    pub values: BTreeMap<String, Vec<ValueEntry>>,
    /// Form fingerprint -> field signature -> value.
    #[serde(default)]
    pub forms: BTreeMap<String, BTreeMap<String, String>>,
    /// Normalised label -> meaning (`None` = not reusable). Permanent.
    #[serde(default)]
    pub label_cache: BTreeMap<String, Option<Meaning>>,
    /// Keys added by the AI (`new:...`).
    #[serde(default)]
    pub extra_keys: Vec<String>,
    /// Remembered people: each person's details kept together.
    #[serde(default)]
    pub profiles: Vec<crate::profiles::Profile>,
    /// Form fingerprint -> field signature -> (person group, role), from the
    /// AI's grouping of that form. Permanent, like the label cache.
    #[serde(default)]
    pub people_cache: BTreeMap<String, BTreeMap<String, (usize, String)>>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub usage: Usage,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `$PDFR_DATA_DIR` (tests, portable installs) or the platform data dir.
pub fn default_path() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("PDFR_DATA_DIR") {
        return Some(PathBuf::from(d).join("memory.json"));
    }
    dirs::data_dir().map(|d| d.join("pdfr").join("memory.json"))
}

impl Store {
    pub fn load(path: &Path) -> Store {
        let mut s: Store = match std::fs::read(path) {
            Err(_) => Store::default(),
            Ok(b) => match serde_json::from_slice(&b) {
                Ok(s) => s,
                Err(_) => {
                    // Never overwrite a file we cannot read (a newer version's
                    // format, damage): keep it aside and start fresh.
                    let aside = path.with_extension(format!("json.unreadable-{}", now()));
                    let _ = std::fs::rename(path, aside);
                    Store::default()
                }
            },
        };
        if s.settings.learning_paused {
            s.settings.learning_paused = false;
            s.settings.remember = Remember::Never;
        }
        s
    }

    /// Writes atomically, with permissions 0600 on Unix.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // A temp file per writer, so two running copies never share one.
        let tmp = path.with_extension(format!(
            "json.tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos())
        ));
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                o.mode(0o600);
            }
            use std::io::Write;
            let mut f = o.open(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Records a value under `key` (use count and date).
    pub fn learn(&mut self, key: &str, value: &str) {
        let v = value.trim();
        if v.is_empty() || v.chars().count() > 200 {
            return;
        }
        let list = self.values.entry(key.to_string()).or_default();
        match list.iter_mut().find(|e| e.value == v) {
            Some(e) => {
                e.count += 1;
                e.last_used = now();
            }
            None => list.push(ValueEntry {
                value: v.to_string(),
                count: 1,
                last_used: now(),
            }),
        }
        list.sort_by(|a, b| b.count.cmp(&a.count).then(b.last_used.cmp(&a.last_used)));
    }

    /// Stored values for a key, most used first.
    pub fn get(&self, key: &str) -> Vec<String> {
        self.values
            .get(key)
            .map(|l| l.iter().map(|e| e.value.clone()).collect())
            .unwrap_or_default()
    }

    pub fn forget_value(&mut self, key: &str, value: &str) {
        for p in &mut self.profiles {
            if p.values.get(key).is_some_and(|v| v == value) {
                p.values.remove(key);
            }
        }
        self.profiles.retain(|p| !p.values.is_empty());
        if let Some(l) = self.values.get_mut(key) {
            l.retain(|e| e.value != value);
            if l.is_empty() {
                self.values.remove(key);
            }
        }
        // Same-form memory holds values as typed ("SE45 5000…",
        // "12/03/1985", "Lindqvist, Anna"): compare their words and digits.
        let tokens = |s: &str| {
            let mut t: Vec<String> = s
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| !w.is_empty())
                .map(str::to_lowercase)
                .collect();
            t.sort();
            t
        };
        let target = tokens(value);
        for f in self.forms.values_mut() {
            f.retain(|_, v| {
                !(v == value || crate::profiles::same_value(v, value) || tokens(v) == target)
            });
        }
    }

    pub fn clear_values(&mut self) {
        self.values.clear();
        self.forms.clear();
        self.profiles.clear();
    }

    pub fn forget_profile(&mut self, id: u64) {
        if let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() {
            for (k, v) in &p.values {
                // The person's values go from the flat lists too, unless
                // another remembered person shares them (an address).
                let shared = self
                    .profiles
                    .iter()
                    .any(|o| o.id != id && o.values.get(k) == Some(v));
                if !shared {
                    self.forget_value(k, v);
                }
            }
        }
        self.profiles.retain(|p| p.id != id);
    }

    pub fn vocabulary(&self) -> Vec<String> {
        let mut v: Vec<String> = crate::meaning::BASE_KEYS
            .iter()
            .map(|s| s.to_string())
            .collect();
        for k in &self.extra_keys {
            if !v.contains(k) {
                v.push(k.clone());
            }
        }
        v
    }
}
