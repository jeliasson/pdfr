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
    /// Model name override for Claude Code; empty means its cheapest model.
    #[serde(default)]
    pub model: String,
    /// The AI tool to use (`ai::Tool::id`); empty: the first one found.
    #[serde(default)]
    pub ai_tool: String,
    /// The model for `ai_tool` when it is not Claude Code; empty: its own
    /// default.
    #[serde(default)]
    pub ai_tool_model: String,
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
    /// New signatures are locked into the page when saving, instead of
    /// being annotations other PDF apps can move.
    #[serde(default)]
    pub lock_signatures: bool,
    /// The page sidebar is shown.
    #[serde(default)]
    pub sidebar: bool,
    /// Pages are shown dark (screen only).
    #[serde(default)]
    pub dark_pages: bool,
    /// Before saving, answers that look wrong are pointed out.
    #[serde(default = "yes")]
    pub check_before_save: bool,
    /// Fillable boxes are tinted on screen (off: the page as other PDF
    /// apps show it).
    #[serde(default = "yes")]
    pub show_fields: bool,
    /// Read the text of scanned pages on this computer (OCR), so search,
    /// copy and suggestions work on scans.
    #[serde(default = "yes")]
    pub read_scans: bool,
    /// Keep a list of recently opened files and where they were left.
    #[serde(default = "yes")]
    pub remember_recent: bool,
    /// Recently opened files, newest first.
    #[serde(default)]
    pub recent: Vec<RecentFile>,
    /// Digital signatures get a trusted timestamp (only a hash is sent to
    /// the timestamp server).
    #[serde(default = "yes")]
    pub sign_timestamp: bool,
    /// The timestamp server; empty means pdfr's default.
    #[serde(default)]
    pub tsa_url: String,
    /// Check whether signers' certificates were revoked, asking the issuer
    /// when the file has no answer; and add those answers to files pdfr
    /// signs (long-term validation).
    #[serde(default = "yes")]
    pub check_revocation: bool,
    /// Reason and place written into new digital signatures.
    #[serde(default)]
    pub sign_reason: String,
    #[serde(default)]
    pub sign_location: String,
    /// The certificate last signed with (SHA-256 fingerprint), offered
    /// first next time. Never a PIN or password.
    #[serde(default)]
    pub last_cert: String,
    /// .p12 / .pfx files signed with before (paths only).
    #[serde(default)]
    pub cert_files: Vec<String>,
    /// AI agents (through `pdfr mcp`) may open and control pdfr windows.
    #[serde(default = "yes")]
    pub agents_enabled: bool,
    /// An agent signing digitally: the user is asked in the window first.
    #[serde(default = "yes")]
    pub agent_ask_sign: bool,
    /// An agent saving over a file that exists: the user is asked first.
    #[serde(default = "yes")]
    pub agent_ask_overwrite: bool,
    /// An agent printing: the user is asked first.
    #[serde(default)]
    pub agent_ask_print: bool,
    /// Agents may be told what pdfr remembers the user usually writes.
    #[serde(default = "yes")]
    pub agent_use_memory: bool,
    /// Web sites whose links open without asking (lowercase host names,
    /// added with "Always open links to …").
    #[serde(default)]
    pub link_hosts: Vec<String>,
}

/// A recently opened file and where it was left.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RecentFile {
    pub path: String,
    /// The page at the top of the window, and how far down it (0..1).
    #[serde(default)]
    pub page: usize,
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub zoom: f32,
    #[serde(default)]
    pub opened_at: u64,
}

/// Most files listed as recent.
pub const RECENT_MAX: usize = 20;

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
            ai_tool: String::new(),
            ai_tool_model: String::new(),
            learning_paused: false,
            remember: Remember::Ask,
            default_app_asked: false,
            update_check: None,
            update_checked_at: 0,
            latest_version: String::new(),
            dismissed_version: String::new(),
            lock_signatures: false,
            sidebar: false,
            dark_pages: false,
            show_fields: true,
            check_before_save: true,
            read_scans: true,
            remember_recent: true,
            recent: vec![],
            sign_timestamp: true,
            tsa_url: String::new(),
            check_revocation: true,
            sign_reason: String::new(),
            sign_location: String::new(),
            last_cert: String::new(),
            cert_files: vec![],
            agents_enabled: true,
            agent_ask_sign: true,
            agent_ask_overwrite: true,
            agent_ask_print: false,
            agent_use_memory: true,
            link_hosts: vec![],
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

/// A signature or initials kept for next time (only when the user asks).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedSignature {
    pub id: u64,
    /// The person it belongs to (`Profile::id`), if chosen.
    #[serde(default)]
    pub person: Option<u64>,
    #[serde(default)]
    pub initials: bool,
    /// The drawing, as `pdfr_core::signature::Ink::encode` text.
    pub ink: String,
    #[serde(default)]
    pub last_used: u64,
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
    /// Signatures and initials saved for next time.
    #[serde(default)]
    pub signatures: Vec<SavedSignature>,
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

    /// Writes the store to `path` after taking in what another writer (a
    /// second pdfr window) changed in the file since `base` was read: what
    /// it added is added here, what it removed is removed here, and our own
    /// changes win. Holds a lock on `path.lock` meanwhile. `base` becomes
    /// what was written.
    pub fn save_merged(&mut self, path: &Path, base: &mut Store) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let _lock = FileLock::take(&path.with_extension("json.lock"));
        let disk: Option<Store> = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        if let Some(disk) = disk
            && &disk != base
        {
            self.merge_from(base, &disk);
        }
        self.save(path)?;
        *base = self.clone();
        Ok(())
    }

    /// Three-way merge: `disk` is `base` plus another writer's changes.
    pub fn merge_from(&mut self, base: &Store, disk: &Store) {
        fn merge_map<K: Ord + Clone, V: Clone + PartialEq>(
            ours: &mut BTreeMap<K, V>,
            base: &BTreeMap<K, V>,
            disk: &BTreeMap<K, V>,
        ) {
            for (k, v) in disk {
                if !base.contains_key(k) && !ours.contains_key(k) {
                    ours.insert(k.clone(), v.clone());
                }
            }
            for k in base.keys() {
                if !disk.contains_key(k) && ours.get(k) == base.get(k) {
                    ours.remove(k);
                }
            }
        }
        // Values: entry by entry within each key.
        for (k, list) in &disk.values {
            let b = base.values.get(k);
            let o = self.values.entry(k.clone()).or_default();
            for e in list {
                let known = |l: Option<&Vec<ValueEntry>>| {
                    l.is_some_and(|l| l.iter().any(|x| x.value == e.value))
                };
                if !known(b) && !o.iter().any(|x| x.value == e.value) {
                    o.push(e.clone());
                }
            }
        }
        for (k, list) in &base.values {
            for e in list {
                let on_disk = disk
                    .values
                    .get(k)
                    .is_some_and(|l| l.iter().any(|x| x.value == e.value));
                if !on_disk && let Some(o) = self.values.get_mut(k) {
                    o.retain(|x| x.value != e.value);
                }
            }
        }
        self.values.retain(|_, l| !l.is_empty());
        merge_map(&mut self.forms, &base.forms, &disk.forms);
        merge_map(&mut self.label_cache, &base.label_cache, &disk.label_cache);
        merge_map(
            &mut self.people_cache,
            &base.people_cache,
            &disk.people_cache,
        );
        // People by id.
        for p in &disk.profiles {
            if !base.profiles.iter().any(|b| b.id == p.id)
                && !self.profiles.iter().any(|o| o.id == p.id)
            {
                self.profiles.push(p.clone());
            }
        }
        self.profiles.retain(|o| {
            let was = base.profiles.iter().any(|b| b.id == o.id);
            !was || disk.profiles.iter().any(|d| d.id == o.id)
        });
        // Signatures by drawing.
        for s in &disk.signatures {
            let new = !base.signatures.iter().any(|b| b.ink == s.ink);
            if new && !self.signatures.iter().any(|o| o.ink == s.ink) {
                let id = self.signatures.iter().map(|x| x.id).max().unwrap_or(0) + 1;
                self.signatures.push(SavedSignature { id, ..s.clone() });
            }
        }
        self.signatures.retain(|o| {
            let was = base.signatures.iter().any(|b| b.ink == o.ink);
            !was || disk.signatures.iter().any(|d| d.ink == o.ink)
        });
        for k in &disk.extra_keys {
            if !self.extra_keys.contains(k) {
                self.extra_keys.push(k.clone());
            }
        }
        // Settings: the other writer's, unless we changed ours.
        if self.settings == base.settings {
            self.settings = disk.settings.clone();
        } else {
            // Recent files: both windows' lists, newest first.
            let mut all = self.settings.recent.clone();
            for r in &disk.settings.recent {
                match all.iter_mut().find(|a| a.path == r.path) {
                    Some(a) if r.opened_at > a.opened_at => *a = r.clone(),
                    Some(_) => {}
                    None => all.push(r.clone()),
                }
            }
            all.sort_by_key(|r| std::cmp::Reverse(r.opened_at));
            all.truncate(RECENT_MAX);
            self.settings.recent = all;
        }
        let u = &mut self.usage;
        u.calls = u.calls.max(disk.usage.calls);
        u.input_tokens = u.input_tokens.max(disk.usage.input_tokens);
        u.output_tokens = u.output_tokens.max(disk.usage.output_tokens);
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
        self.signatures.clear();
    }

    /// Keeps a signature (or initials) for next time; returns its id.
    pub fn add_signature(&mut self, person: Option<u64>, initials: bool, ink: String) -> u64 {
        // The same drawing saved twice is kept once.
        if let Some(s) = self
            .signatures
            .iter_mut()
            .find(|s| s.ink == ink && s.initials == initials)
        {
            s.person = person.or(s.person);
            s.last_used = now();
            return s.id;
        }
        let id = self.signatures.iter().map(|s| s.id).max().unwrap_or(0) + 1;
        self.signatures.push(SavedSignature {
            id,
            person,
            initials,
            ink,
            last_used: now(),
        });
        id
    }

    pub fn touch_signature(&mut self, id: u64) {
        if let Some(s) = self.signatures.iter_mut().find(|s| s.id == id) {
            s.last_used = now();
        }
    }

    pub fn forget_signature(&mut self, id: u64) {
        self.signatures.retain(|s| s.id != id);
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
        self.signatures.retain(|s| s.person != Some(id));
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

/// An advisory lock on a file, held while it lives (Unix; elsewhere a
/// no-op). Two pdfr windows never write the store at the same moment.
struct FileLock {
    #[cfg(unix)]
    _file: Option<std::fs::File>,
}

impl FileLock {
    fn take(path: &Path) -> FileLock {
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let f = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)
                .ok();
            if let Some(f) = &f {
                // Blocks briefly if the other window is writing.
                unsafe {
                    libc::flock(f.as_raw_fd(), libc::LOCK_EX);
                }
            }
            FileLock { _file: f }
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            FileLock {}
        }
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    #[test]
    fn two_windows_keep_each_others_changes() {
        let dir = std::env::temp_dir().join(format!("pdfr-merge-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("memory.json");
        let mut start = Store::default();
        start.learn("email", "old@example.se");
        start.signatures.push(SavedSignature {
            id: 1,
            person: None,
            initials: false,
            ink: "ink1 2".into(),
            last_used: 0,
        });
        let mut base = Store::default();
        start.save_merged(&path, &mut base).unwrap();

        // Two windows load the same file.
        let (mut a, mut a_base) = (Store::load(&path), Store::load(&path));
        let (mut b, mut b_base) = (Store::load(&path), Store::load(&path));
        a.learn("email", "anna@example.se");
        a.forget_signature(1);
        a.save_merged(&path, &mut a_base).unwrap();
        b.learn("phone", "0701234567");
        b.settings.dark_pages = true;
        b.save_merged(&path, &mut b_base).unwrap();

        let disk = Store::load(&path);
        assert!(
            disk.get("email").contains(&"anna@example.se".to_string()),
            "A's value kept"
        );
        assert!(disk.get("email").contains(&"old@example.se".to_string()));
        assert_eq!(
            disk.get("phone"),
            vec!["0701234567".to_string()],
            "B's value"
        );
        assert!(disk.signatures.is_empty(), "A's delete is not undone by B");
        assert!(disk.settings.dark_pages);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn link_hosts_load_from_old_files() {
        // Settings written before the field existed load with none allowed.
        let s: Settings = serde_json::from_str(r#"{"dark_pages": true}"#).unwrap();
        assert!(s.link_hosts.is_empty());
        let mut s = s;
        s.link_hosts.push("example.org".into());
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.link_hosts, vec!["example.org".to_string()]);
    }
}
