//! pdfr assist: field labels, label → meaning, the AI backend, the value
//! memory and suggestions. No UI.
//!
//! Three layers, each useful alone: labels (no AI), meanings (dictionary,
//! cache, then AI for what is left), and value memory (no AI).

pub mod ai;
pub mod dates;
pub mod labels;
pub mod meaning;
pub mod memory;
pub mod profiles;

use ai::{Answer, Backend};
use labels::FieldInfo;
use meaning::{Meaning, Part, normalise};
use memory::Store;
use pdfr_core::edit::sanitize;
use pdfr_core::field::{Field, FieldKind, Origin};
use pdfr_core::geom::PageGeom;
use pdfr_core::text::TextRun;
use profiles::{Person, Profile};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

/// Per-document analysis: labels, groups and meanings.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub info: Vec<FieldInfo>,
    pub meanings: Vec<Option<Meaning>>,
    /// Field indices whose label is still unknown after dictionary and cache.
    pub unresolved: Vec<usize>,
    pub fingerprint: String,
    /// Per field: which person on the form it is about.
    pub person: Vec<Option<usize>>,
    pub people: Vec<Person>,
    /// Section headings and, per field, its section; row per field.
    pub sections: Vec<String>,
    pub section_of: Vec<Option<usize>>,
    pub rows: Vec<usize>,
    /// The grouping came from the AI (cached), not only local rules.
    pub people_from_ai: bool,
}

/// A request to group a multi-person form's fields into people.
#[derive(Clone, Debug)]
pub struct PeopleRequest {
    pub fingerprint: String,
    /// Field signature per request line.
    pub sigs: Vec<String>,
    pub prompt: String,
}

#[derive(Clone, Debug)]
pub struct PeopleResult {
    pub fingerprint: String,
    pub sigs: Vec<String>,
    pub groups: Vec<Option<(usize, String)>>,
}

/// One AI request, ready to run on a background thread.
pub struct AiJob {
    pub labels: Vec<String>,
    pub vocab: Vec<String>,
    pub backend: Arc<dyn Backend>,
    /// Group a multi-person form's fields into people (labels and section
    /// headings only).
    pub people: Option<PeopleRequest>,
}

#[derive(Debug, Default)]
pub struct AiOutcome {
    /// `(normalised label, answer)`; `None` answer: the model gave none.
    pub answers: Vec<(String, Option<Answer>)>,
    /// Per call: (labels, input tokens, output tokens).
    pub usage: Vec<(usize, u64, u64)>,
    pub calls: u32,
    pub error: Option<String>,
    /// The exact request texts sent (for tests and logs).
    pub requests: Vec<String>,
    /// The raw replies received.
    pub replies: Vec<String>,
    pub people: Option<PeopleResult>,
}

impl AiJob {
    /// Runs the job: one call per 150 labels, at most one retry per call.
    pub fn run(self) -> AiOutcome {
        let mut out = AiOutcome::default();
        for chunk in self.labels.chunks(ai::MAX_LABELS_PER_CALL) {
            let prompt = ai::build_prompt(chunk, &self.vocab);
            let mut parsed = None;
            for _attempt in 0..2 {
                out.calls += 1;
                out.requests.push(prompt.clone());
                match self.backend.run(ai::INSTRUCTION, &prompt) {
                    Ok(reply) => {
                        out.replies.push(reply.text.clone());
                        out.usage.push((
                            chunk.len(),
                            reply.input_tokens.unwrap_or(0),
                            reply.output_tokens.unwrap_or(0),
                        ));
                        match ai::parse_reply(&reply.text, chunk.len(), &self.vocab) {
                            Some(p) => {
                                parsed = Some(p);
                                break;
                            }
                            None => {
                                out.error = Some("the reply did not parse and was discarded".into())
                            }
                        }
                    }
                    Err(e) => out.error = Some(e),
                }
            }
            if let Some(p) = parsed {
                for (l, a) in chunk.iter().zip(p) {
                    out.answers.push((normalise(l), a));
                }
            } else {
                break; // never loop: give up on the rest for now
            }
        }
        if let Some(req) = self.people {
            let n = req.sigs.len();
            for _attempt in 0..2 {
                out.calls += 1;
                out.requests.push(req.prompt.clone());
                match self.backend.run(profiles::PEOPLE_INSTRUCTION, &req.prompt) {
                    Ok(reply) => {
                        out.replies.push(reply.text.clone());
                        out.usage.push((
                            n,
                            reply.input_tokens.unwrap_or(0),
                            reply.output_tokens.unwrap_or(0),
                        ));
                        match profiles::parse_people_reply(&reply.text, n) {
                            Some(groups) => {
                                out.people = Some(PeopleResult {
                                    fingerprint: req.fingerprint.clone(),
                                    sigs: req.sigs.clone(),
                                    groups,
                                });
                                break;
                            }
                            None => {
                                out.error = Some("the reply did not parse and was discarded".into())
                            }
                        }
                    }
                    Err(e) => out.error = Some(e),
                }
            }
        }
        out
    }
}

pub struct Assistant {
    pub store: Store,
    /// The store as last read from or written to the file, to tell what
    /// another running pdfr changed there since (see `Store::save_merged`).
    base: Store,
    path: Option<PathBuf>,
    backend: Option<Arc<dyn Backend>>,
}

/// FNV-1a: a stable hash for fingerprints.
fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

fn coarse(f: &Field) -> &'static str {
    if f.kind.is_toggle() { "x" } else { "t" }
}

/// A field's position signature within its form (tolerant to small shifts).
pub fn field_sig(f: &Field) -> String {
    let r = |v: f32| (v / 3.0).round() as i32;
    format!(
        "p{}:{}:{}:{}:{}:{}",
        f.page,
        r(f.rect.x0),
        r(f.rect.y0),
        r(f.rect.x1),
        r(f.rect.y1),
        coarse(f)
    )
}

/// Fingerprint of a form: page sizes plus field positions and kinds.
pub fn fingerprint(fields: &[Field], pages: &[PageGeom]) -> String {
    let mut s = String::new();
    for p in pages {
        let (w, h) = p.display_size();
        s.push_str(&format!("{}x{};", w.round(), h.round()));
    }
    let mut sigs: Vec<String> = fields
        .iter()
        .filter(|f| f.origin != Origin::Manual)
        .map(field_sig)
        .collect();
    sigs.sort();
    s.push_str(&sigs.join(";"));
    fnv(&s)
}

/// A label asking for a secret: password, PIN, card security code.
pub fn is_secret_label(label: &str) -> bool {
    let n = normalise(label);
    let words: Vec<&str> = n.split_whitespace().collect();
    const WORDS: &[&str] = &[
        "password",
        "passcode",
        "passwort",
        "kennwort",
        "pin",
        "pinkod",
        "pincode",
        "cvv",
        "cvc",
        "cvv2",
        "cid",
        "contrasena",
        "clave",
        "losenord",
        "motdepasse",
        "nip",
    ];
    const PHRASES: &[&str] = &[
        "mot de passe",
        "security code",
        "codigo de seguridad",
        "card verification",
        "sakerhetskod",
    ];
    words.iter().any(|w| WORDS.contains(w)) || PHRASES.iter().any(|p| n.contains(p))
}

/// 13–19 digits passing the Luhn check: a payment card number. Never stored.
pub fn is_card_number(v: &str) -> bool {
    let digits: Vec<u32> = v
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_digit(10))
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn split_name(full: &str) -> Option<(String, String)> {
    let w: Vec<&str> = full.split_whitespace().collect();
    (w.len() == 2).then(|| (w[0].to_string(), w[1].to_string()))
}

fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect()
}

impl Assistant {
    /// Loads the store from the user data directory and finds an AI tool.
    pub fn open_default() -> Assistant {
        Self::open_at(memory::default_path())
    }

    /// Loads the store from `path` (if any) and finds an AI tool.
    pub fn open_at(path: Option<PathBuf>) -> Assistant {
        let store = path.as_deref().map(Store::load).unwrap_or_default();
        let model = store.settings.model.clone();
        let backend = ai::detect(Some(&model)).map(|b| Arc::new(b) as Arc<dyn Backend>);
        Assistant {
            base: store.clone(),
            store,
            path,
            backend,
        }
    }

    /// For tests: a given store, optional file path and backend.
    pub fn new(
        store: Store,
        path: Option<PathBuf>,
        backend: Option<Arc<dyn Backend>>,
    ) -> Assistant {
        Assistant {
            base: store.clone(),
            store,
            path,
            backend,
        }
    }

    pub fn set_backend(&mut self, b: Option<Arc<dyn Backend>>) {
        self.backend = b;
    }

    pub fn redetect_backend(&mut self) {
        let model = self.store.settings.model.clone();
        self.backend = ai::detect(Some(&model)).map(|b| Arc::new(b) as Arc<dyn Backend>);
    }

    pub fn backend_name(&self) -> Option<String> {
        self.backend.as_ref().map(|b| b.name())
    }

    /// Writes the store, first taking in what another running pdfr (a
    /// second window) saved meanwhile.
    pub fn save(&mut self) {
        if let Some(p) = &self.path {
            let _ = self.store.save_merged(p, &mut self.base);
        }
    }

    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }

    /// AI may be called now (enabled, consented, tool present).
    pub fn ai_allowed(&self) -> bool {
        self.store.settings.ai_enabled
            && self.store.settings.ai_consent == Some(true)
            && self.backend.is_some()
    }

    /// A call would be made but the user has not been asked yet.
    pub fn needs_consent(&self) -> bool {
        self.store.settings.ai_enabled
            && self.store.settings.ai_consent.is_none()
            && self.backend.is_some()
    }

    fn resolve(&self, label: &str) -> Option<Option<Meaning>> {
        if let Some(r) = meaning::dictionary(label) {
            return Some(r);
        }
        self.store.label_cache.get(&normalise(label)).cloned()
    }

    /// Labels, groups and meanings for every field.
    pub fn analyse(
        &self,
        fields: &[Field],
        text: &HashMap<usize, Vec<TextRun>>,
        pages: &[PageGeom],
    ) -> Analysis {
        let info = labels::label_fields(fields, text);
        let mut meanings: Vec<Option<Meaning>> = vec![None; fields.len()];
        let mut unresolved = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            if f.kind.is_toggle() || f.kind.is_signature() || f.kind.is_password() {
                continue;
            }
            let Some(label) = &info[i].label else {
                continue;
            };
            match self.resolve(label) {
                Some(m) => meanings[i] = m,
                None => {
                    // A group asks once, through its first member.
                    if info[i].group.as_ref().is_none_or(|g| g.index == 0) {
                        unresolved.push(i);
                    }
                }
            }
        }
        // Group members share the group's meaning; date groups get parts.
        let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (i, inf) in info.iter().enumerate() {
            if let Some(g) = &inf.group {
                groups.entry(g.group).or_default().push(i);
            }
        }
        for members in groups.values() {
            let Some(m) = members.iter().find_map(|&i| meanings[i].clone()) else {
                continue;
            };
            let label = info[members[0]].label.clone().unwrap_or_default();
            let parts: Option<Vec<Part>> = if m.is_date() && members.len() == 3 {
                meaning::date_order(&label).or_else(|| {
                    let caps: Vec<usize> = members
                        .iter()
                        .map(|&i| fields[i].capacity().unwrap_or(0))
                        .collect();
                    // Year first means year-month-day (ISO); otherwise the
                    // year is last or in the middle of day/month.
                    match caps.iter().position(|&c| c == 4)? {
                        0 => Some(vec![Part::Year, Part::Month, Part::Day]),
                        1 => Some(vec![Part::Month, Part::Year, Part::Day]),
                        _ => Some(vec![Part::Day, Part::Month, Part::Year]),
                    }
                })
            } else {
                None
            };
            for (k, &i) in members.iter().enumerate() {
                let mut mm = m.clone();
                if let Some(p) = &parts {
                    mm.part = p.get(k).copied();
                }
                meanings[i] = Some(mm);
            }
        }
        let fp = fingerprint(fields, pages);
        let rows = profiles::rows(fields);
        let (sections, section_of) = profiles::sections(fields, &info, text);
        let mut person = profiles::group_people(fields, &meanings, &info);
        let mut people: Vec<Person> =
            vec![Person::default(); person.iter().flatten().max().map_or(0, |m| m + 1)];
        let mut people_from_ai = false;
        // The AI's grouping of this form, if it was asked before.
        if let Some(cache) = self.store.people_cache.get(&fp) {
            let mut remap: BTreeMap<usize, usize> = BTreeMap::new();
            let mut ai_people: Vec<Person> = Vec::new();
            let mut ai_person = vec![None; fields.len()];
            for (i, f) in fields.iter().enumerate() {
                let Some((g, role)) = cache.get(&field_sig(f)) else {
                    continue;
                };
                if *g == 0 {
                    continue;
                }
                let k = *remap.entry(*g).or_insert_with(|| {
                    ai_people.push(Person {
                        role: Some(role.clone()),
                    });
                    ai_people.len() - 1
                });
                ai_person[i] = Some(k);
            }
            if !ai_people.is_empty() {
                // Person fields the AI did not see keep no group.
                for (i, p) in ai_person.iter().enumerate() {
                    if person[i].is_some() {
                        person[i] = *p;
                    }
                }
                people = ai_people;
                people_from_ai = true;
            }
        }
        Analysis {
            info,
            meanings,
            unresolved,
            fingerprint: fp,
            person,
            people,
            sections,
            section_of,
            rows,
            people_from_ai,
        }
    }

    /// The AI request to group this form's fields into people: only for
    /// forms with more than one person, once per form, labels and section
    /// headings only.
    pub fn people_request(&self, an: &Analysis, fields: &[Field]) -> Option<PeopleRequest> {
        if an.people.len() < 2
            || an.people_from_ai
            || self.store.people_cache.contains_key(&an.fingerprint)
        {
            return None;
        }
        let mut sigs = Vec::new();
        let mut lines = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            if an.person.get(i).copied().flatten().is_none() {
                continue;
            }
            let Some(label) = an.info[i]
                .label
                .as_ref()
                .filter(|l| labels::plausible_label(l))
            else {
                continue;
            };
            sigs.push(field_sig(f));
            lines.push((an.section_of[i], an.rows[i], label.clone()));
        }
        if sigs.len() < 2 {
            return None;
        }
        // A heading containing anything the user has stored is never sent.
        let known: Vec<String> = self
            .store
            .values
            .values()
            .flatten()
            .map(|e| normalise(&e.value))
            .filter(|v| v.len() >= 4)
            .collect();
        let safe = |k: usize| {
            let h = normalise(&an.sections[k]);
            !known.iter().any(|v| h.contains(v.as_str()))
        };
        for l in &mut lines {
            if l.0.is_some_and(|k| !safe(k)) {
                l.0 = None;
            }
        }
        let used: Vec<usize> = {
            let mut v: Vec<usize> = lines.iter().filter_map(|l| l.0).collect();
            v.sort();
            v.dedup();
            v
        };
        // Number only the sections that are used, in order.
        let names: Vec<String> = used.iter().map(|&k| an.sections[k].clone()).collect();
        let lines: Vec<(Option<usize>, usize, String)> = lines
            .into_iter()
            .map(|(sec, row, l)| (sec.and_then(|k| used.iter().position(|&u| u == k)), row, l))
            .collect();
        let prompt = profiles::build_people_prompt(&names, &lines);
        Some(PeopleRequest {
            fingerprint: an.fingerprint.clone(),
            sigs,
            prompt,
        })
    }

    pub fn make_people_job(&self, req: PeopleRequest) -> Option<AiJob> {
        if !self.ai_allowed() {
            return None;
        }
        Some(AiJob {
            labels: vec![],
            vocab: vec![],
            backend: self.backend.clone()?,
            people: Some(req),
        })
    }

    /// Unknown labels to send: deduplicated, each label at most once ever.
    pub fn pending_labels(&self, an: &Analysis) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for &i in &an.unresolved {
            let Some(l) = &an.info[i].label else { continue };
            // Second guard: only label-like text ever leaves the machine.
            if !labels::plausible_label(l) || l.chars().filter(|c| c.is_alphabetic()).count() < 3 {
                continue;
            }
            let n = normalise(l);
            if n.is_empty() || self.store.label_cache.contains_key(&n) || !seen.insert(n) {
                continue;
            }
            out.push(ai::prepare_label(l));
        }
        out
    }

    pub fn make_job(&self, labels: Vec<String>) -> Option<AiJob> {
        if labels.is_empty() || !self.ai_allowed() {
            return None;
        }
        Some(AiJob {
            labels,
            vocab: self.store.vocabulary(),
            backend: self.backend.clone()?,
            people: None,
        })
    }

    /// Stores AI answers permanently and records token use.
    pub fn apply(&mut self, out: &AiOutcome) {
        for (label, ans) in &out.answers {
            let m = match ans {
                Some(Answer::Key(k)) => {
                    let mut parts = k.split('|');
                    let base = parts.next().unwrap_or("");
                    let ff = parts.any(|p| p == "ff");
                    let (key, part) = match base.split_once('.') {
                        Some((k, "day")) => (k, Some(Part::Day)),
                        Some((k, "month")) => (k, Some(Part::Month)),
                        Some((k, "year")) => (k, Some(Part::Year)),
                        _ => (base, None),
                    };
                    Some(Meaning {
                        key: key.to_string(),
                        part,
                        family_first: ff,
                    })
                }
                Some(Answer::New(k)) => {
                    if !self.store.extra_keys.contains(k) {
                        self.store.extra_keys.push(k.clone());
                    }
                    Some(Meaning::new(k))
                }
                Some(Answer::None) | None => None,
            };
            self.store.label_cache.insert(label.clone(), m);
        }
        if let Some(p) = &out.people {
            let map: BTreeMap<String, (usize, String)> = p
                .sigs
                .iter()
                .zip(&p.groups)
                .filter_map(|(s, g)| g.clone().map(|g| (s.clone(), g)))
                .collect();
            self.store.people_cache.insert(p.fingerprint.clone(), map);
        }
        for &(n, i, o) in &out.usage {
            self.store.usage.calls += 1;
            self.store.usage.input_tokens += i;
            self.store.usage.output_tokens += o;
            self.store.usage.log.push((n, i, o));
        }
        self.save();
    }

    /// Suggestions for field `i`, most confident first. Fit to the field.
    pub fn suggestions(&self, an: &Analysis, fields: &[Field], i: usize) -> Vec<String> {
        let f = &fields[i];
        if f.read_only || f.kind.is_toggle() || f.kind.is_signature() || f.kind.is_password() {
            return vec![];
        }
        let same_form = self
            .store
            .forms
            .get(&an.fingerprint)
            .and_then(|m| m.get(&field_sig(f)))
            .cloned();
        let person = an.person.get(i).copied().flatten();
        // With remembered people, same-form memory only helps choose the
        // person (see `profile_order`); it never overrides that choice.
        let has_person_values = person.is_some() && !self.store.profiles.is_empty();
        let mut raw: Vec<String> = Vec::new();
        // Same-form memory first (it needs no labels, so it covers scans),
        // unless this person's other fields already say who it is.
        if !has_person_values && let Some(v) = &same_form {
            raw.push(v.clone());
        }
        if let Some(m) = an.meanings.get(i).cloned().flatten() {
            match self.profile_order(an, fields, i) {
                Some(order) => {
                    for pr in &order {
                        let get =
                            |k: &str| pr.values.get(k).cloned().into_iter().collect::<Vec<_>>();
                        raw.extend(self.values_for(&m, an, fields, i, &get));
                    }
                    // Details no profile holds yet (learned before profiles).
                    if !has_person_values && order.iter().all(|p| !p.values.contains_key(&m.key)) {
                        raw.extend(self.values_for(&m, an, fields, i, &|k| self.store.get(k)));
                    }
                }
                None => raw.extend(self.values_for(&m, an, fields, i, &|k| self.store.get(k))),
            }
        }
        // Last resort: what was here before, unless the person's other
        // fields show it is someone else now.
        let entered = person.is_some_and(|p| !self.person_values(an, fields, p, i).is_empty());
        if has_person_values
            && !entered
            && let Some(v) = same_form
        {
            raw.push(v);
        }
        let mut out: Vec<String> = Vec::new();
        for v in raw {
            if let Some(v) = fit(f, &v)
                && !out.contains(&v)
            {
                out.push(v);
            }
        }
        out.truncate(8);
        out
    }

    /// Canonical details already entered for person `p` (other than field
    /// `except`): what tells which remembered person this is.
    fn person_values(
        &self,
        an: &Analysis,
        fields: &[Field],
        p: usize,
        except: usize,
    ) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (j, f) in fields.iter().enumerate() {
            if j == except
                || an.person.get(j).copied().flatten() != Some(p)
                || f.value.trim().is_empty()
                || an.info[j].group.is_some()
            {
                continue;
            }
            let Some(m) = an.meanings.get(j).cloned().flatten() else {
                continue;
            };
            out.extend(self.canon(&m, &f.value, an.info[j].label.as_deref().unwrap_or("")));
        }
        with_derived_names(out.into_iter().collect())
            .into_iter()
            .collect()
    }

    /// Remembered people in the order to offer them for field `i`: those
    /// matching what is already entered for this person first (never one
    /// that contradicts it), then by role, then by use. `None` when the
    /// field is not about a person or no profiles exist yet.
    fn profile_order(&self, an: &Analysis, fields: &[Field], i: usize) -> Option<Vec<&Profile>> {
        let p = an.person.get(i).copied().flatten()?;
        if self.store.profiles.is_empty() {
            return None;
        }
        let vals = self.person_values(an, fields, p, i);
        let role = an.people.get(p).and_then(|x| x.role.clone());
        // Who was in this place when the same form was saved before.
        let before: Vec<(String, String)> = match self.store.forms.get(&an.fingerprint) {
            Some(m) => {
                let mut v = Vec::new();
                for (j, f) in fields.iter().enumerate() {
                    if an.person.get(j).copied().flatten() != Some(p) || an.info[j].group.is_some()
                    {
                        continue;
                    }
                    if let (Some(val), Some(mm)) =
                        (m.get(&field_sig(f)), an.meanings.get(j).cloned().flatten())
                    {
                        v.extend(self.canon(&mm, val, an.info[j].label.as_deref().unwrap_or("")));
                    }
                }
                with_derived_names(v.into_iter().collect())
                    .into_iter()
                    .collect()
            }
            None => vec![],
        };
        // Profiles already chosen for other people on this form.
        let mut claimed: Vec<u64> = Vec::new();
        for q in 0..an.people.len() {
            if q == p {
                continue;
            }
            let qv = self.person_values(an, fields, q, usize::MAX);
            if qv.is_empty() {
                continue;
            }
            for pr in &self.store.profiles {
                let (hit, conflict) = compare(pr, &qv);
                if hit && !conflict {
                    claimed.push(pr.id);
                }
            }
        }
        let mut cands: Vec<(i64, &Profile)> = Vec::new();
        for pr in &self.store.profiles {
            let (hit, conflict) = compare(pr, &vals);
            // Only identifying details (name, ID, birth date, e-mail) can
            // rule a person out; a phone or address alone does not.
            if conflict || (has_identifying(&vals) && !hit) {
                continue;
            }
            let mut score = pr.count as i64;
            if hit {
                score += 10_000;
            }
            if role.as_ref().is_some_and(|r| pr.roles.contains(r)) {
                score += 1_000;
            }
            let (was_here, other_then) = compare(pr, &before);
            if was_here && !other_then {
                score += 2_000;
            }
            if claimed.contains(&pr.id) {
                score -= 5_000;
            }
            cands.push((score, pr));
        }
        cands.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.last_used.cmp(&a.1.last_used)));
        Some(cands.into_iter().map(|c| c.1).collect())
    }

    /// Values for every empty field, person by person, so that different
    /// people on the form get different remembered people.
    pub fn fill_all(&self, an: &Analysis, fields: &[Field]) -> Vec<(usize, String)> {
        let mut sim: Vec<Field> = fields.to_vec();
        let mut out = Vec::new();
        let mut order: Vec<usize> = (0..fields.len()).collect();
        // People in order of appearance, then fields not about a person.
        order.sort_by_key(|&i| (an.person.get(i).copied().flatten().unwrap_or(usize::MAX), i));
        for i in order {
            let f = &sim[i];
            if !f.is_empty() || f.read_only || f.kind.is_toggle() || f.kind.is_signature() {
                continue;
            }
            if let Some(v) = self.confident(an, &sim, i) {
                sim[i].value = v.clone();
                out.push((i, v));
            }
        }
        out
    }

    fn values_for(
        &self,
        m: &Meaning,
        an: &Analysis,
        fields: &[Field],
        i: usize,
        get: &dyn Fn(&str) -> Vec<String>,
    ) -> Vec<String> {
        let f = &fields[i];
        let label = an.info[i].label.clone().unwrap_or_default();
        let mut v: Vec<String> = Vec::new();
        match m.key.as_str() {
            "full_name" if m.family_first => {
                // Family name first: from the separate parts when known
                // (right for two surnames), else from a two-word name.
                for g in get("given_name") {
                    for fam in get("family_name") {
                        v.push(format!("{fam}, {g}"));
                    }
                }
                for n in get("full_name") {
                    if let Some((g, fam)) = split_name(&n) {
                        v.push(format!("{fam}, {g}"));
                    }
                }
            }
            "full_name" => {
                let mut names = get("full_name");
                for g in get("given_name") {
                    for fam in get("family_name") {
                        let n = format!("{g} {fam}");
                        if !names.contains(&n) {
                            names.push(n);
                        }
                    }
                }
                v = names;
            }
            "given_name" | "family_name" => {
                v = get(&m.key);
                for n in get("full_name") {
                    if let Some((g, fam)) = split_name(&n) {
                        v.push(if m.key == "given_name" { g } else { fam });
                    }
                }
            }
            "signature_date" => {
                v.push(dates::format_for(dates::today(), m.part, f, &label));
            }
            k if m.is_date() => {
                for d in get(k) {
                    if let Some(ymd) = dates::parse_iso(&d) {
                        v.push(dates::format_for(ymd, m.part, f, &label));
                    }
                }
            }
            k => v = get(k),
        }
        // Spread across one-character boxes; slice for split groups.
        if f.kind.is_comb() && !m.is_date() {
            let cap = f.capacity().unwrap_or(usize::MAX);
            let group = an.info[i].group.clone();
            v = v
                .into_iter()
                .filter_map(|val| {
                    let mut a = alnum(&val);
                    match &group {
                        Some(g) => {
                            // Members in index order with their capacities.
                            let members: Vec<usize> = (0..fields.len())
                                .filter(|&j| {
                                    an.info[j]
                                        .group
                                        .as_ref()
                                        .is_some_and(|h| h.group == g.group)
                                })
                                .collect();
                            let mut ordered = members.clone();
                            ordered.sort_by_key(|&j| an.info[j].group.as_ref().unwrap().index);
                            let total: usize = ordered
                                .iter()
                                .map(|&j| fields[j].capacity().unwrap_or(0))
                                .sum();
                            if a.chars().count() == total + 2
                                && (a.starts_with("19") || a.starts_with("20"))
                            {
                                a = a.chars().skip(2).collect();
                            }
                            if a.chars().count() > total {
                                return None;
                            }
                            let off: usize = ordered
                                .iter()
                                .take(g.index)
                                .map(|&j| fields[j].capacity().unwrap_or(0))
                                .sum();
                            let part: String = a.chars().skip(off).take(cap).collect();
                            (!part.is_empty()).then_some(part)
                        }
                        None => {
                            // Swedish personal numbers: drop the century to fit 10 boxes.
                            if a.chars().count() == cap + 2
                                && (a.starts_with("19") || a.starts_with("20"))
                            {
                                a = a.chars().skip(2).collect();
                            }
                            Some(a)
                        }
                    }
                })
                .collect();
        }
        v
    }

    /// The suggestion to show as ghost text / use for "Fill all", if confident.
    pub fn confident(&self, an: &Analysis, fields: &[Field], i: usize) -> Option<String> {
        self.suggestions(an, fields, i).into_iter().next()
    }

    /// What saving would remember, by person, for the user to choose from.
    /// Excluded up front: passwords, card numbers, checkboxes and fields
    /// marked "don't remember" (`skip`).
    pub fn plan_learning(&self, an: &Analysis, fields: &[Field], skip: &[bool]) -> LearnPlan {
        let mut groups: Vec<LearnGroup> = Vec::new();
        let mut slot: BTreeMap<Option<usize>, usize> = BTreeMap::new();
        let never = Self::never_store(an, fields);
        // A form about one person: everything on it is shown under them (it
        // is their application). Only personal details join their profile;
        // the rest (dates, reference numbers) is remembered for this form.
        let only = (an.people.len() == 1).then_some(0);
        for (i, f) in fields.iter().enumerate() {
            if never[i] || !Self::learnable(f, skip.get(i).copied().unwrap_or(false)) {
                continue;
            }
            let person = an.person.get(i).copied().flatten().or(only);
            let g = *slot.entry(person).or_insert_with(|| {
                groups.push(LearnGroup {
                    person,
                    title: String::new(),
                    items: vec![],
                });
                groups.len() - 1
            });
            let value = f.value.trim();
            let same = |it: &LearnItem| same_value(&it.value, value);
            if let Some(it) = groups[g].items.iter_mut().find(|it| same(it)) {
                it.also.push(i);
                continue;
            }
            let label = an.info[i].label.clone().unwrap_or_else(|| "Field".into());
            groups[g].items.push(LearnItem {
                field: i,
                also: vec![],
                label,
                value: f.value.trim().to_string(),
                selected: true,
            });
        }
        for g in &mut groups {
            g.title = match g.person {
                Some(p) => {
                    let vals = self.person_values(an, fields, p, usize::MAX);
                    let name = vals
                        .iter()
                        .find(|(k, _)| k == "full_name")
                        .map(|(_, v)| v.clone());
                    let role = an.people.get(p).and_then(|x| x.role.clone());
                    match (name, role) {
                        (Some(n), Some(r)) => format!("{n} ({r})"),
                        (Some(n), None) => n,
                        (None, Some(r)) => capitalise(&r),
                        (None, None) => format!("Person {}", p + 1),
                    }
                }
                None => "Other details".into(),
            };
        }
        // People first, in order; other details last.
        groups.sort_by_key(|g| g.person.unwrap_or(usize::MAX));
        // The same person found more than once (the name repeated on every
        // page starts a new person each time): one group, each value once.
        let mut merged: Vec<LearnGroup> = Vec::new();
        for g in groups {
            let named = g.person.is_some() && !g.title.starts_with("Person ");
            let into = merged
                .iter_mut()
                .find(|m| named && m.person.is_some() && same_value(&m.title, &g.title));
            match into {
                Some(m) => {
                    for it in g.items {
                        match m.items.iter_mut().find(|x| same_value(&x.value, &it.value)) {
                            Some(x) => {
                                x.also.push(it.field);
                                x.also.extend(it.also);
                            }
                            None => m.items.push(it),
                        }
                    }
                }
                None => merged.push(g),
            }
        }
        LearnPlan { groups: merged }
    }

    fn learnable(f: &Field, skip: bool) -> bool {
        !(skip
            || f.kind.is_toggle()
            || f.kind.is_signature()
            || f.kind.is_password()
            || f.value.trim().is_empty()
            || is_card_number(&f.value))
    }

    /// Fields whose values are never stored, beyond the per-field checks:
    /// boxes labelled as secrets (passwords, PINs, card codes — detected
    /// fields carry no password flag), and rows of boxes that together hold
    /// a card number.
    fn never_store(an: &Analysis, fields: &[Field]) -> Vec<bool> {
        let mut out: Vec<bool> = (0..fields.len())
            .map(|i| {
                an.info[i].label.as_deref().is_some_and(is_secret_label)
                    || Self::passing_date(an, fields, i)
            })
            .collect();
        let mut groups: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        for (i, inf) in an.info.iter().enumerate() {
            if let Some(g) = &inf.group {
                groups.entry(g.group).or_default().push((g.index, i));
            }
        }
        for mut members in groups.into_values() {
            members.sort();
            let joined: String = members
                .iter()
                .map(|&(_, i)| fields[i].value.trim())
                .collect();
            let secret = members.iter().any(|&(_, i)| out[i]);
            if secret || is_card_number(&joined) {
                for (_, i) in members {
                    out[i] = true;
                }
            }
        }
        out
    }

    /// A date that belongs to this one filling (today's date, the day of an
    /// application), not to a person: worth nothing on the next form, so it
    /// is not offered or stored. Personal dates (birth, expiry) are kept.
    fn passing_date(an: &Analysis, fields: &[Field], i: usize) -> bool {
        match an.meanings.get(i).and_then(|m| m.as_ref()) {
            Some(m) if m.key == "signature_date" => true,
            Some(_) => false,
            // Unknown or not reusable: a value written as a date.
            None => looks_like_date(&fields[i].value),
        }
    }

    /// Remembers the selected items of a plan.
    pub fn commit(&mut self, an: &Analysis, fields: &[Field], plan: &LearnPlan) {
        let mut skip = vec![true; fields.len()];
        for g in &plan.groups {
            for it in &g.items {
                if it.selected {
                    for &f in std::iter::once(&it.field).chain(&it.also) {
                        if f < skip.len() {
                            skip[f] = false;
                        }
                    }
                }
            }
        }
        self.learn(an, fields, &skip);
    }

    /// Learns what the user typed (called on save). `skip[i]`: not to be
    /// remembered (marked "don't remember", or deselected when saving).
    pub fn learn(&mut self, an: &Analysis, fields: &[Field], skip: &[bool]) {
        if self.store.settings.remember == memory::Remember::Never {
            return;
        }
        let mut form: BTreeMap<String, String> = BTreeMap::new();
        let mut groups: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        // Canonical (key, value) per person (None: not about a person).
        let mut pairs: Vec<(Option<usize>, String, String)> = Vec::new();
        let never = Self::never_store(an, fields);
        let skip: Vec<bool> = (0..fields.len())
            .map(|i| never[i] || skip.get(i).copied().unwrap_or(false))
            .collect();
        let skip = &skip[..];
        for (i, f) in fields.iter().enumerate() {
            if !Self::learnable(f, skip.get(i).copied().unwrap_or(false)) {
                continue;
            }
            form.insert(field_sig(f), f.value.clone());
            let Some(m) = an.meanings.get(i).cloned().flatten() else {
                continue;
            };
            if let Some(g) = &an.info[i].group {
                groups.entry(g.group).or_default().push((g.index, i));
                continue;
            }
            let person = an.person.get(i).copied().flatten();
            for (k, v) in self.canon(&m, &f.value, &an.info[i].label.clone().unwrap_or_default()) {
                pairs.push((person, k, v));
            }
        }
        for (_, mut members) in groups {
            members.sort();
            let first = members[0].1;
            let person = an.person.get(first).copied().flatten();
            let Some(m) = an.meanings[first].clone() else {
                continue;
            };
            if m.is_date() {
                let (mut d, mut mo, mut y) = (None, None, None);
                for &(_, i) in &members {
                    let v: u32 = match fields[i].value.trim().parse() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    match an.meanings[i].as_ref().and_then(|m| m.part) {
                        Some(Part::Day) => d = Some(v),
                        Some(Part::Month) => mo = Some(v),
                        Some(Part::Year) => {
                            y = Some(if v < 100 { dates::expand_year(v) } else { v })
                        }
                        None => {}
                    }
                }
                if let (Some(d), Some(mo), Some(y)) = (d, mo, y)
                    && dates::valid(y, mo, d)
                    && m.key != "signature_date"
                {
                    pairs.push((person, m.key.clone(), dates::iso((y, mo, d))));
                }
            } else {
                // Only a fully remembered group is a whole value.
                if members.iter().any(|&(_, i)| {
                    skip.get(i).copied().unwrap_or(false) || fields[i].value.trim().is_empty()
                }) {
                    continue;
                }
                let joined: String = members
                    .iter()
                    .map(|&(_, i)| fields[i].value.clone())
                    .collect();
                for (k, v) in self.canon(&Meaning::new(&m.key), &joined, "") {
                    pairs.push((person, k, v));
                }
            }
        }
        for (_, k, v) in &pairs {
            self.store.learn(k, v);
        }
        // Each person's details together, in a profile.
        let mut by_person: BTreeMap<usize, BTreeMap<String, String>> = BTreeMap::new();
        for (p, k, v) in pairs {
            if let Some(p) = p
                && profiles::is_person_key(&k)
            {
                by_person.entry(p).or_default().insert(k, v);
            }
        }
        for (p, vals) in by_person {
            let vals = with_derived_names(vals);
            let role = an.people.get(p).and_then(|x| x.role.clone());
            self.remember_person(vals, role);
        }
        if !form.is_empty() {
            self.store
                .forms
                .entry(an.fingerprint.clone())
                .or_default()
                .extend(form);
        }
        self.save();
    }

    /// Updates the matching profile (same ID, else same name, else same
    /// e-mail) or creates one for a new person.
    fn remember_person(&mut self, vals: BTreeMap<String, String>, role: Option<String>) {
        let get = |k: &str| vals.get(k).map(String::as_str);
        let find = |key: &str, profiles: &[Profile]| -> Option<usize> {
            let v = get(key)?;
            profiles.iter().position(|p| {
                p.values
                    .get(key)
                    .is_some_and(|w| profiles::same_value(v, w))
            })
        };
        let ps = &self.store.profiles;
        // Never merge into someone whose ID or birth date differs.
        let ident: Vec<(String, String)> = ["national_id", "passport_number", "date_of_birth"]
            .iter()
            .filter_map(|k| vals.get(*k).map(|v| (k.to_string(), v.clone())))
            .collect();
        let find =
            |key: &str, ps: &[Profile]| find(key, ps).filter(|&i| !compare(&ps[i], &ident).1);
        let hit = find("national_id", ps)
            .or_else(|| find("passport_number", ps))
            .or_else(|| find("full_name", ps))
            .or_else(|| {
                // Same e-mail, and no different name.
                let i = find("email", ps)?;
                let name_differs = get("full_name")
                    .zip(ps[i].values.get("full_name"))
                    .is_some_and(|(a, b)| !profiles::same_value(a, b));
                (!name_differs).then_some(i)
            });
        let now = memory::now();
        match hit {
            Some(i) => {
                let p = &mut self.store.profiles[i];
                p.values.extend(vals);
                if let Some(r) = role
                    && !p.roles.contains(&r)
                {
                    p.roles.push(r);
                }
                p.count += 1;
                p.last_used = now;
            }
            None => {
                if !vals.keys().any(|k| profiles::ANCHORS.contains(&k.as_str())) {
                    return; // not enough to tell who this is
                }
                let id = self.store.profiles.iter().map(|p| p.id).max().unwrap_or(0) + 1;
                self.store.profiles.push(Profile {
                    id,
                    values: vals,
                    roles: role.into_iter().collect(),
                    count: 1,
                    last_used: now,
                });
            }
        }
    }

    /// A typed value in canonical form: names given name first, dates ISO,
    /// IBANs without spaces. Empty for values never remembered.
    fn canon(&self, m: &Meaning, value: &str, label: &str) -> Vec<(String, String)> {
        let v = value.trim();
        if v.is_empty() || v.chars().count() > 200 {
            return vec![];
        }
        match m.key.as_str() {
            "signature_date" => vec![], // always today; nothing to remember
            "full_name" if m.family_first => {
                if let Some((fam, given)) = v.split_once(',') {
                    let (fam, given) = (fam.trim(), given.trim());
                    vec![
                        ("full_name".into(), format!("{given} {fam}")),
                        ("given_name".into(), given.into()),
                        ("family_name".into(), fam.into()),
                    ]
                } else if let Some((a, b)) = split_name(v) {
                    vec![("full_name".into(), format!("{b} {a}"))]
                } else if !v.contains(' ') {
                    vec![("full_name".into(), v.to_string())]
                } else {
                    // "GARCÍA LÓPEZ ANA": where the surnames end is unknown;
                    // storing a guess would corrupt the person's name.
                    vec![]
                }
            }
            k if m.is_date() => {
                if m.part.is_none()
                    && let Some(ymd) = dates::parse(v, meaning::date_order(label).as_deref())
                {
                    return vec![(k.to_string(), dates::iso(ymd))];
                }
                vec![]
            }
            "iban" => vec![("iban".into(), alnum(v).to_uppercase())],
            k => vec![(k.to_string(), v.to_string())],
        }
    }
}

/// What saving would remember, grouped by person.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LearnPlan {
    pub groups: Vec<LearnGroup>,
}

impl LearnPlan {
    pub fn is_empty(&self) -> bool {
        self.groups.iter().all(|g| g.items.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LearnGroup {
    pub person: Option<usize>,
    pub title: String,
    pub items: Vec<LearnItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LearnItem {
    pub field: usize,
    /// Other fields holding the same value for the same person (the name
    /// repeated on every page): shown once, remembered or not together.
    pub also: Vec<usize>,
    pub label: String,
    pub value: String,
    pub selected: bool,
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// Adds full name from given + family name and the other way round, so
/// people match however a form asks for their name.
fn with_derived_names(mut v: BTreeMap<String, String>) -> BTreeMap<String, String> {
    if !v.contains_key("full_name")
        && let (Some(g), Some(f)) = (v.get("given_name"), v.get("family_name"))
    {
        let n = format!("{g} {f}");
        v.insert("full_name".into(), n);
    }
    if let Some(n) = v.get("full_name").cloned()
        && let Some((g, f)) = split_name(&n)
    {
        v.entry("given_name".into()).or_insert(g);
        v.entry("family_name".into()).or_insert(f);
    }
    v
}

const IDENTIFYING: &[&str] = &[
    "full_name",
    "given_name",
    "family_name",
    "national_id",
    "passport_number",
    "date_of_birth",
    "email",
];

fn has_identifying(vals: &[(String, String)]) -> bool {
    vals.iter().any(|(k, _)| IDENTIFYING.contains(&k.as_str()))
}

/// Does a profile match these details (`hit`), or contradict them
/// (`conflict`)? Only identifying keys count.
fn compare(p: &Profile, vals: &[(String, String)]) -> (bool, bool) {
    let (mut hit, mut conflict) = (false, false);
    for (k, v) in vals {
        if !IDENTIFYING.contains(&k.as_str()) {
            continue;
        }
        if let Some(w) = p.values.get(k) {
            if profiles::same_value(v, w) {
                hit = true;
            } else {
                conflict = true;
            }
        }
    }
    (hit, conflict)
}

/// Makes a value fit a field: comb boxes take capitals without spaces, and
/// nothing longer than the field accepts.
pub fn fit(f: &Field, v: &str) -> Option<String> {
    let mut s = sanitize(f, v);
    if f.kind.is_comb() {
        s = s.to_uppercase();
    }
    if s.is_empty() {
        return None;
    }
    if let Some(cap) = f.capacity()
        && s.chars().count() > cap
    {
        return None;
    }
    if let FieldKind::Choice { options, editable } = &f.kind {
        if !editable
            && !options
                .iter()
                .any(|(e, d)| e.eq_ignore_ascii_case(&s) || d.eq_ignore_ascii_case(&s))
        {
            return None;
        }
        if let Some((e, _)) = options
            .iter()
            .find(|(e, d)| e.eq_ignore_ascii_case(&s) || d.eq_ignore_ascii_case(&s))
        {
            return Some(e.clone());
        }
    }
    Some(s)
}

/// "20/12/2026", "2026-12-20", "20.12.26": three numbers with separators
/// that make a valid date.
fn looks_like_date(v: &str) -> bool {
    let v = v.trim();
    let parts = v
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .count();
    parts == 3
        && v.chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '/' | '-' | '.' | ' '))
        && dates::parse(v, None).is_some()
}

/// The same value typed twice: equal ignoring case and spacing.
fn same_value(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    norm(a) == norm(b)
}
