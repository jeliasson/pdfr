//! Autocomplete end to end, with a fake AI backend so tests are free and
//! repeatable.

use pdfr_assist::ai::{Backend, Reply};
use pdfr_assist::memory::Store;
use pdfr_assist::{Analysis, Assistant, labels};
use pdfr_core::field::{Field, Origin, TextMode};
use pdfr_core::geom::{PageGeom, Rect};
use pdfr_core::pipeline::{Loaded, load};
use pdfr_core::text::TextRun;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

fn sample(name: &str) -> Loaded {
    load(
        std::fs::read(format!(
            "{}/../../samples/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap(),
    )
    .unwrap()
}

/// A scripted backend that records every request.
struct Fake {
    reply: Box<dyn Fn(&str) -> String + Send + Sync>,
    calls: AtomicUsize,
    requests: Mutex<Vec<String>>,
}

impl Fake {
    fn new(reply: impl Fn(&str) -> String + Send + Sync + 'static) -> Arc<Fake> {
        Arc::new(Fake {
            reply: Box::new(reply),
            calls: AtomicUsize::new(0),
            requests: Mutex::new(vec![]),
        })
    }
}

impl Backend for Fake {
    fn name(&self) -> String {
        "fake".into()
    }
    fn run(&self, instruction: &str, prompt: &str) -> Result<Reply, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .unwrap()
            .push(format!("{instruction}\n{prompt}"));
        Ok(Reply {
            text: (self.reply)(prompt),
            input_tokens: Some(prompt.len() as u64 / 4),
            output_tokens: Some(10),
        })
    }
}

/// Answers every label `none`, except known test labels.
fn smart(prompt: &str) -> String {
    prompt
        .lines()
        .filter_map(|l| {
            l.split_once(':')
                .filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|(n, label)| {
            let key = match label {
                l if l.contains("titular") => "full_name|ff",
                l if l.contains("Kontoinnehavare") => "full_name",
                l if l.contains("pet") => "new:pet_name",
                _ => "none",
            };
            format!("{n}:{key}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn consented(store: Store) -> Store {
    let mut s = store;
    s.settings.ai_consent = Some(true);
    s
}

/// A synthetic one-page form: text fields with labels to their left.
fn synthetic(labels: &[&str]) -> (Vec<Field>, HashMap<usize, Vec<TextRun>>, Vec<PageGeom>) {
    let mut fields = vec![];
    let mut runs = vec![];
    for (i, l) in labels.iter().enumerate() {
        let y = 100.0 + i as f32 * 30.0;
        fields.push(Field::new_text(
            0,
            Rect::new(250.0, y, 500.0, y + 18.0),
            TextMode::Single,
            Origin::Detected,
        ));
        runs.push(TextRun {
            rect: Rect::new(50.0, y + 5.0, 240.0, y + 15.0),
            text: l.to_string(),
            size: 9.0,
            xs: vec![],
        });
    }
    (
        fields,
        HashMap::from([(0, runs)]),
        vec![PageGeom::new([0.0, 0.0, 595.0, 842.0], 0)],
    )
}

fn run_ai(a: &mut Assistant, an: &Analysis) -> usize {
    let labels = a.pending_labels(an);
    match a.make_job(labels) {
        Some(job) => {
            let out = job.run();
            a.apply(&out);
            out.calls as usize
        }
        None => 0,
    }
}

#[test]
fn three_name_labels_one_stored_name() {
    let mut store = Store::default();
    store.learn("full_name", "Anna Lindqvist");
    let a = Assistant::new(store, None, None);
    let (fields, text, pages) =
        synthetic(&["Firstname + surname", "Full name", "Apellidos y Nombre"]);
    let an = a.analyse(&fields, &text, &pages);
    let keys: Vec<_> = an
        .meanings
        .iter()
        .map(|m| m.as_ref().map(|m| m.key.clone()))
        .collect();
    assert_eq!(keys, vec![Some("full_name".to_string()); 3]);
    assert_eq!(
        a.confident(&an, &fields, 0).as_deref(),
        Some("Anna Lindqvist")
    );
    assert_eq!(
        a.confident(&an, &fields, 1).as_deref(),
        Some("Anna Lindqvist")
    );
    // The Spanish label asks for the family name first.
    assert_eq!(
        a.confident(&an, &fields, 2).as_deref(),
        Some("Lindqvist, Anna")
    );
}

#[test]
fn names_are_derived_both_ways() {
    let mut store = Store::default();
    store.learn("given_name", "Anna");
    store.learn("family_name", "Lindqvist");
    let a = Assistant::new(store, None, None);
    let (fields, text, pages) = synthetic(&["Full name", "Nombre", "Efternamn"]);
    let an = a.analyse(&fields, &text, &pages);
    assert_eq!(
        a.confident(&an, &fields, 0).as_deref(),
        Some("Anna Lindqvist")
    );
    assert_eq!(a.confident(&an, &fields, 1).as_deref(), Some("Anna"));
    assert_eq!(a.confident(&an, &fields, 2).as_deref(), Some("Lindqvist"));
    // And the reverse: given/family from a stored full name.
    let mut store = Store::default();
    store.learn("full_name", "Erik Svensson");
    let a = Assistant::new(store, None, None);
    let (fields, text, pages) = synthetic(&["First name", "Surname"]);
    let an = a.analyse(&fields, &text, &pages);
    assert_eq!(a.confident(&an, &fields, 0).as_deref(), Some("Erik"));
    assert_eq!(a.confident(&an, &fields, 1).as_deref(), Some("Svensson"));
}

#[test]
fn stored_date_fills_three_part_group_and_iban_fills_boxes() {
    let mut store = Store::default();
    store.learn("date_of_birth", "1985-03-12");
    store.learn("national_id", "198503121234");
    let a = Assistant::new(store, None, None);
    let l = sample("flat_form.pdf");
    let an = a.analyse(&l.fields, &l.text, &l.form.pages);
    let at = |x: f32, y: f32| {
        l.fields
            .iter()
            .position(|f| (f.rect.x0 - x).abs() < 2.5 && (f.rect.y0 - y).abs() < 2.5)
            .unwrap()
    };
    let (d, m, y) = (at(190.0, 232.0), at(232.0, 232.0), at(274.0, 232.0));
    assert_eq!(
        an.info[d].label.as_deref(),
        Some("Date of birth (DD MM YYYY)")
    );
    assert_eq!(
        an.info[y].group.as_ref().map(|g| (g.index, g.count)),
        Some((2, 3))
    );
    assert_eq!(a.confident(&an, &l.fields, d).as_deref(), Some("12"));
    assert_eq!(a.confident(&an, &l.fields, m).as_deref(), Some("03"));
    assert_eq!(a.confident(&an, &l.fields, y).as_deref(), Some("1985"));
    // Personal number: 10 boxes; the 12-digit stored value drops its century.
    let pn = at(150.0, 200.0);
    assert_eq!(
        a.confident(&an, &l.fields, pn).as_deref(),
        Some("8503121234")
    );

    // IBAN across a row of boxes, and split over several rows.
    let mut store = Store::default();
    store.learn("iban", "SE45 5000 0000 0583 9825 7466");
    let a = Assistant::new(store, None, None);
    let comb = |x0: f32, n: usize| {
        let r = Rect::new(x0, 100.0, x0 + 12.0 * n as f32, 116.0);
        Field::new_text(
            0,
            r,
            TextMode::Comb(Field::equal_cells(&r, n)),
            Origin::Detected,
        )
    };
    let pages = vec![PageGeom::new([0.0, 0.0, 595.0, 842.0], 0)];
    let label = |x: f32| {
        HashMap::from([(
            0,
            vec![TextRun {
                rect: Rect::new(x, 104.0, x + 30.0, 113.0),
                text: "IBAN".into(),
                size: 9.0,
                xs: vec![],
            }],
        )])
    };
    let one = vec![comb(60.0, 24)];
    let an = a.analyse(&one, &label(20.0), &pages);
    assert_eq!(
        a.confident(&an, &one, 0).as_deref(),
        Some("SE4550000000058398257466")
    );
    let split: Vec<Field> = (0..6).map(|i| comb(60.0 + i as f32 * 54.0, 4)).collect();
    let an = a.analyse(&split, &label(20.0), &pages);
    let parts: Vec<String> = (0..6)
        .map(|i| a.confident(&an, &split, i).unwrap())
        .collect();
    assert_eq!(parts, vec!["SE45", "5000", "0000", "0583", "9825", "7466"]);
}

#[test]
fn one_ai_call_per_form_and_none_when_known() {
    let fake = Fake::new(smart);
    let mut a = Assistant::new(consented(Store::default()), None, Some(fake.clone()));
    let (fields, text, pages) = synthetic(&[
        "Nombre del titular",
        "Kontoinnehavare",
        "Your pet's name",
        "Full name",
        "Nombre del titular",
    ]);
    // First open: one call with the unknown labels, deduplicated.
    let an = a.analyse(&fields, &text, &pages);
    assert_eq!(run_ai(&mut a, &an), 1);
    let req = fake.requests.lock().unwrap()[0].clone();
    assert_eq!(
        req.matches("Nombre del titular").count(),
        1,
        "deduplicated: {req}"
    );
    assert!(
        !req.contains("Full name"),
        "dictionary labels are never sent"
    );
    // The answers are used straight away (family name first from |ff).
    let mut store_name = a.store.clone();
    store_name.learn("full_name", "Anna Lindqvist");
    a.store = store_name;
    let an = a.analyse(&fields, &text, &pages);
    assert_eq!(
        a.confident(&an, &fields, 0).as_deref(),
        Some("Lindqvist, Anna")
    );
    assert_eq!(
        a.confident(&an, &fields, 1).as_deref(),
        Some("Anna Lindqvist")
    );
    assert!(a.store.vocabulary().contains(&"pet_name".to_string()));
    // Second open of the same form: zero calls.
    let an = a.analyse(&fields, &text, &pages);
    assert_eq!(run_ai(&mut a, &an), 0);
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        1,
        "exactly one call in total"
    );
    // A form whose labels are all in the dictionary: none.
    let l = sample("flat_form.pdf");
    let an = a.analyse(&l.fields, &l.text, &l.form.pages);
    assert!(
        a.pending_labels(&an).is_empty(),
        "{:?}",
        a.pending_labels(&an)
    );
    assert_eq!(run_ai(&mut a, &an), 0);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    assert_eq!(a.store.usage.calls, 1);
}

#[test]
fn requests_never_contain_values() {
    let fake = Fake::new(smart);
    let mut store = consented(Store::default());
    store.learn("full_name", "Secret Person");
    let mut a = Assistant::new(store, None, Some(fake.clone()));
    let (mut fields, text, pages) = synthetic(&[
        "Nombre del titular",
        "Account holder reference",
        "Your pet's name",
    ]);
    fields[0].value = "Anna Lindqvist".into();
    fields[1].value = "REF-778899".into();
    fields[2].value = "Fido".into();
    let an = a.analyse(&fields, &text, &pages);
    run_ai(&mut a, &an);
    let reqs = fake.requests.lock().unwrap().clone();
    assert!(!reqs.is_empty());
    for r in &reqs {
        for secret in ["Anna Lindqvist", "REF-778899", "Fido", "Secret Person"] {
            assert!(!r.contains(secret), "request contains a value: {r}");
        }
    }
}

#[test]
fn malformed_reply_is_discarded_without_loop() {
    let fake = Fake::new(|_| "Sure! I'd be happy to help with those labels.".into());
    let mut a = Assistant::new(consented(Store::default()), None, Some(fake.clone()));
    let (fields, text, pages) = synthetic(&["Nombre del titular", "Kontoinnehavare"]);
    let an = a.analyse(&fields, &text, &pages);
    let labels = a.pending_labels(&an);
    let out = a.make_job(labels).unwrap().run();
    assert!(out.answers.is_empty());
    assert!(out.error.is_some());
    assert_eq!(
        fake.calls.load(Ordering::SeqCst),
        2,
        "one try plus at most one retry"
    );
    a.apply(&out);
    assert!(
        a.store.label_cache.is_empty(),
        "nothing from a bad reply is stored"
    );
}

#[test]
fn works_without_ai_and_same_form_memory_covers_scans() {
    for ai_on in [false, true] {
        let mut store = Store::default();
        store.settings.ai_enabled = ai_on;
        // No tool installed (backend None), or AI turned off.
        let mut a = Assistant::new(store, None, None);
        let l = sample("scanned_form.pdf");
        assert!(
            l.text.values().all(|t| t.is_empty()),
            "a scan has no text layer"
        );
        let an = a.analyse(&l.fields, &l.text, &l.form.pages);
        assert!(an.info.iter().all(|i| i.label.is_none()));
        assert!(a.make_job(a.pending_labels(&an)).is_none());
        // Fill and "save": learn.
        let mut fields = l.fields.clone();
        let big = fields
            .iter()
            .position(|f| (f.rect.x0 - 151.0).abs() < 3.0 && (f.rect.y0 - 168.0).abs() < 3.0)
            .unwrap();
        fields[big].value = "Anna Lindqvist".into();
        a.learn(&an, &fields, &vec![false; fields.len()]);
        // Open the same scanned form again: the value is offered for that field.
        let l2 = sample("scanned_form.pdf");
        let an2 = a.analyse(&l2.fields, &l2.text, &l2.form.pages);
        assert_eq!(an2.fingerprint, an.fingerprint);
        assert_eq!(
            a.confident(&an2, &l2.fields, big).as_deref(),
            Some("Anna Lindqvist")
        );
    }
}

#[test]
fn learning_rules() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut fields, text, pages) =
        synthetic(&["Apellidos y Nombre", "Card number", "Email", "Password"]);
    fields[0].value = "Lindqvist, Anna".into();
    fields[1].value = "4111 1111 1111 1111".into();
    fields[2].value = "anna@example.se".into();
    if let pdfr_core::field::FieldKind::Text { password, .. } = &mut fields[3].kind {
        *password = true;
    }
    fields[3].value = "hunter2".into();
    let an = a.analyse(&fields, &text, &pages);
    a.learn(&an, &fields, &[false, false, true, false]);
    // Family-first names are stored in the canonical order.
    assert_eq!(a.store.get("full_name"), vec!["Anna Lindqvist"]);
    // Card numbers, "don't remember" fields and passwords are never stored.
    let all = serde_json::to_string(&a.store).unwrap();
    assert!(!all.contains("4111"));
    assert!(!all.contains("anna@example.se"));
    assert!(!all.contains("hunter2"));
    // Paused learning stores nothing.
    let mut b = Assistant::new(Store::default(), None, None);
    b.store.settings.remember = pdfr_assist::memory::Remember::Never;
    b.learn(&an, &fields, &[false; 4]);
    assert!(b.store.values.is_empty());
    let _ = labels::clean("x");
}

#[test]
fn store_file_is_private() {
    let dir = std::env::temp_dir().join(format!("pdfr-store-{}", std::process::id()));
    let p = dir.join("memory.json");
    let mut s = Store::default();
    s.learn("email", "a@b.c");
    s.save(&p).unwrap();
    assert_eq!(Store::load(&p), s);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Two people on one form: a heading, then name, ID, address and phone each.
fn two_people_form() -> (Vec<Field>, HashMap<usize, Vec<TextRun>>, Vec<PageGeom>) {
    let labels = [
        "Family name",
        "Given name",
        "Personal number",
        "Street address",
        "Phone",
    ];
    let mut fields = vec![];
    let mut runs = vec![];
    for (s, heading) in ["1. DETAILS OF THE CHILD", "2. DETAILS OF THE APPLICANT"]
        .iter()
        .enumerate()
    {
        let top = 100.0 + s as f32 * 220.0;
        runs.push(TextRun {
            rect: Rect::new(50.0, top, 300.0, top + 12.0),
            text: heading.to_string(),
            size: 11.0,
            xs: vec![],
        });
        for (i, l) in labels.iter().enumerate() {
            let y = top + 25.0 + i as f32 * 30.0;
            fields.push(Field::new_text(
                0,
                Rect::new(250.0, y, 500.0, y + 18.0),
                TextMode::Single,
                Origin::Detected,
            ));
            runs.push(TextRun {
                rect: Rect::new(50.0, y + 5.0, 240.0, y + 15.0),
                text: l.to_string(),
                size: 9.0,
                xs: vec![],
            });
        }
    }
    (
        fields,
        HashMap::from([(0, runs)]),
        vec![PageGeom::new([0.0, 0.0, 595.0, 842.0], 0)],
    )
}

fn fill(fields: &mut [Field], from: usize, vals: &[&str]) {
    for (k, v) in vals.iter().enumerate() {
        fields[from + k].value = v.to_string();
    }
}

#[test]
fn each_person_keeps_their_own_details() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut fields, text, pages) = two_people_form();
    let an = a.analyse(&fields, &text, &pages);
    assert_eq!(an.people.len(), 2, "a repeated name starts a second person");
    assert_eq!(an.person[0], Some(0));
    assert_eq!(an.person[9], Some(1));
    fill(
        &mut fields,
        0,
        &["Lind", "Elsa", "20190101-1111", "Storgatan 1", "0701111111"],
    );
    fill(
        &mut fields,
        5,
        &["Berg", "Per", "19800202-2222", "Lillgatan 2", "0702222222"],
    );
    a.learn(&an, &fields, &[false; 10]);
    assert_eq!(a.store.profiles.len(), 2);
    assert_eq!(a.store.profiles[0].values["street_address"], "Storgatan 1");
    assert_eq!(a.store.profiles[1].values["full_name"], "Per Berg");

    // A fresh copy: entering Per in the first section offers Per's details
    // there, and Elsa's (not Per's again) in the second.
    let (mut fresh, text, pages) = two_people_form();
    fresh[1].value = "Per".into();
    let an = a.analyse(&fresh, &text, &pages);
    assert_eq!(a.confident(&an, &fresh, 3).as_deref(), Some("Lillgatan 2"));
    assert_eq!(a.confident(&an, &fresh, 4).as_deref(), Some("0702222222"));
    assert_eq!(a.confident(&an, &fresh, 1 + 5).as_deref(), Some("Elsa"));
    // Someone new: no other person's details are offered for them.
    fresh[5].value = "Svensson".into();
    let an = a.analyse(&fresh, &text, &pages);
    assert_eq!(
        a.confident(&an, &fresh, 8),
        None,
        "no address for an unknown person"
    );

    // Fill all gives the two people two different remembered people.
    let (empty, text, pages) = two_people_form();
    let an = a.analyse(&empty, &text, &pages);
    let filled: HashMap<usize, String> = a.fill_all(&an, &empty).into_iter().collect();
    assert_ne!(filled[&3], filled[&8], "two people, two addresses");
    let first_is_elsa = filled[&1] == "Elsa";
    assert_eq!(
        filled[&3],
        if first_is_elsa {
            "Storgatan 1"
        } else {
            "Lillgatan 2"
        },
        "address matches the name"
    );

    // Learning the same person again updates their profile.
    let (mut again, text, pages) = two_people_form();
    fill(
        &mut again,
        0,
        &["Lind", "Elsa", "20190101-1111", "Nygatan 3", ""],
    );
    let an = a.analyse(&again, &text, &pages);
    a.learn(&an, &again, &[false; 10]);
    assert_eq!(a.store.profiles.len(), 2);
    assert_eq!(a.store.profiles[0].values["street_address"], "Nygatan 3");
}

#[test]
fn ai_groups_people_from_labels_and_headings_only() {
    let fake = Fake::new(|prompt: &str| {
        // First five fields: the child; the rest: the applicant.
        prompt
            .lines()
            .filter_map(|l| l.split('|').next().and_then(|n| n.parse::<usize>().ok()))
            .map(|n| {
                if n <= 5 {
                    format!("{n}:1:child")
                } else {
                    format!("{n}:2:applicant")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    });
    let mut a = Assistant::new(consented(Store::default()), None, Some(fake.clone()));
    let (mut fields, text, pages) = two_people_form();
    fill(
        &mut fields,
        0,
        &["Lind", "Elsa", "20190101-1111", "Storgatan 1", "0701111111"],
    );
    let an = a.analyse(&fields, &text, &pages);
    let req = a
        .people_request(&an, &fields)
        .expect("two people: ask once");
    assert!(req.prompt.contains("DETAILS OF THE CHILD"));
    assert!(req.prompt.contains("Street address"));
    for v in ["Lind", "Elsa", "20190101", "Storgatan", "0701111111"] {
        assert!(!req.prompt.contains(v), "request contains a value: {v}");
    }
    let out = a.make_people_job(req).unwrap().run();
    assert_eq!(out.calls, 1);
    a.apply(&out);
    let an = a.analyse(&fields, &text, &pages);
    assert!(an.people_from_ai);
    assert_eq!(an.people[0].role.as_deref(), Some("child"));
    assert_eq!(an.people[1].role.as_deref(), Some("applicant"));
    // Cached: never asked again for this form.
    assert!(a.people_request(&an, &fields).is_none());
    // Roles are remembered with the person.
    a.learn(&an, &fields, &[false; 10]);
    assert_eq!(a.store.profiles[0].roles, vec!["child".to_string()]);
    // One person on a form: no call at all.
    let (one, text1, pages1) = synthetic(&["Full name", "Email"]);
    let an1 = a.analyse(&one, &text1, &pages1);
    assert!(a.people_request(&an1, &one).is_none());
}

#[test]
fn save_plan_lists_values_by_person_and_respects_choices() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut fields, text, pages) = two_people_form();
    fill(
        &mut fields,
        0,
        &["Lind", "Elsa", "20190101-1111", "Storgatan 1", "0701111111"],
    );
    fill(
        &mut fields,
        5,
        &["Berg", "Per", "19800202-2222", "Lillgatan 2", "0702222222"],
    );
    let an = a.analyse(&fields, &text, &pages);
    let mut plan = a.plan_learning(&an, &fields, &[false; 10]);
    assert_eq!(plan.groups.len(), 2);
    assert_eq!(plan.groups[0].title, "Elsa Lind");
    assert_eq!(plan.groups[0].items.len(), 5);
    // Deselect Per entirely and Elsa's phone.
    for it in &mut plan.groups[1].items {
        it.selected = false;
    }
    plan.groups[0].items[4].selected = false;
    a.commit(&an, &fields, &plan);
    let all = serde_json::to_string(&a.store).unwrap();
    assert!(all.contains("Storgatan 1"));
    assert!(!all.contains("0701111111"), "deselected value stored");
    assert!(
        !all.contains("Berg") && !all.contains("Lillgatan"),
        "deselected person stored"
    );
    assert_eq!(a.store.profiles.len(), 1);
}

#[test]
fn one_person_form_lists_everything_under_that_person() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut fields, text, pages) = synthetic(&["Full name", "Email", "Reference number"]);
    fill(
        &mut fields,
        0,
        &["Rasmus Kjellberg", "r@example.com", "REF-77"],
    );
    let an = a.analyse(&fields, &text, &pages);
    let plan = a.plan_learning(&an, &fields, &[false; 3]);
    assert_eq!(plan.groups.len(), 1, "no separate \"Other details\" group");
    assert_eq!(plan.groups[0].title, "Rasmus Kjellberg");
    assert_eq!(plan.groups[0].items.len(), 3);
    a.commit(&an, &fields, &plan);
    // The reference number is not a personal detail: kept for the form only.
    let p = &a.store.profiles[0];
    assert!(p.values.values().all(|v| v != "REF-77"), "{:?}", p.values);
}

#[test]
fn passing_dates_are_not_offered_but_birth_dates_are() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut fields, text, pages) =
        synthetic(&["Full name", "Date of birth", "Date", "Application day"]);
    fill(
        &mut fields,
        0,
        &["Rasmus Kjellberg", "12/04/1988", "20/12/2026", "21/12/2026"],
    );
    let an = a.analyse(&fields, &text, &pages);
    let plan = a.plan_learning(&an, &fields, &[false; 4]);
    let values: Vec<&str> = plan
        .groups
        .iter()
        .flat_map(|g| &g.items)
        .map(|i| i.value.as_str())
        .collect();
    assert_eq!(
        values,
        vec!["Rasmus Kjellberg", "12/04/1988"],
        "today's and one-off dates are left out"
    );
    // "Remember everything" does not store them either.
    a.learn(&an, &fields, &[false; 4]);
    let all = serde_json::to_string(&a.store).unwrap();
    assert!(all.contains("1988") && !all.contains("2026"), "{all}");
}

#[test]
fn repeated_values_are_listed_once() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut fields, text, pages) = synthetic(&["Full name", "Email", "Name", "Full name"]);
    fill(
        &mut fields,
        0,
        &[
            "Rasmus Kjellberg",
            "r@example.com",
            "rasmus  kjellberg",
            "Rasmus Kjellberg",
        ],
    );
    let an = a.analyse(&fields, &text, &pages);
    let mut plan = a.plan_learning(&an, &fields, &[false; 4]);
    let items: Vec<_> = plan.groups.iter().flat_map(|g| &g.items).collect();
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!(items[0].also, vec![2, 3]);
    // Deselecting the one row leaves out every copy.
    plan.groups[0].items[0].selected = false;
    a.commit(&an, &fields, &plan);
    let all = serde_json::to_string(&a.store).unwrap().to_lowercase();
    assert!(
        !all.contains("kjellberg") && all.contains("r@example.com"),
        "{all}"
    );
}

fn comb_row(x0: f32, y: f32, n: usize) -> Field {
    let r = Rect::new(x0, y, x0 + 12.0 * n as f32, y + 16.0);
    Field::new_text(
        0,
        r,
        TextMode::Comb(Field::equal_cells(&r, n)),
        Origin::Detected,
    )
}

fn label_at(x: f32, y: f32, t: &str) -> TextRun {
    TextRun {
        rect: Rect::new(x, y + 4.0, x + 30.0, y + 13.0),
        text: t.into(),
        size: 9.0,
        xs: vec![],
    }
}

#[test]
fn secrets_are_never_stored_even_on_flat_forms() {
    let pages = vec![PageGeom::new([0.0, 0.0, 595.0, 842.0], 0)];
    // A card number typed across four rows of four boxes (labelled "IBAN"
    // so it would otherwise be learned), and a detected "PIN" box.
    let mut fields: Vec<Field> = (0..4)
        .map(|i| comb_row(60.0 + i as f32 * 54.0, 100.0, 4))
        .collect();
    for (i, v) in ["4111", "1111", "1111", "1111"].iter().enumerate() {
        fields[i].value = v.to_string();
    }
    let mut pin = Field::new_text(
        0,
        Rect::new(60.0, 150.0, 200.0, 166.0),
        TextMode::Single,
        Origin::Detected,
    );
    pin.value = "8642".into();
    fields.push(pin);
    let text = HashMap::from([(
        0,
        vec![label_at(20.0, 100.0, "IBAN"), label_at(20.0, 150.0, "PIN")],
    )]);
    let mut a = Assistant::new(Store::default(), None, None);
    let an = a.analyse(&fields, &text, &pages);
    assert!(
        a.plan_learning(&an, &fields, &[false; 5]).is_empty(),
        "nothing offered for remembering"
    );
    a.learn(&an, &fields, &[false; 5]);
    let all = serde_json::to_string(&a.store).unwrap();
    assert!(!all.contains("4111") && !all.contains("8642"), "{all}");
}

#[test]
fn same_name_different_id_is_two_people() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut f1, text, pages) = synthetic(&["Full name", "Personal number", "Street address"]);
    f1[0].value = "John Smith".into();
    f1[1].value = "111".into();
    f1[2].value = "Elm St 1".into();
    let an = a.analyse(&f1, &text, &pages);
    a.learn(&an, &f1, &[false; 3]);
    let mut f2 = f1.clone();
    f2[1].value = "222".into();
    f2[2].value = "Oak St 2".into();
    a.learn(&an, &f2, &[false; 3]);
    assert_eq!(a.store.profiles.len(), 2, "different IDs: different people");
    assert_eq!(a.store.profiles[0].values["street_address"], "Elm St 1");
}

#[test]
fn year_first_date_boxes_and_phone_first_typing() {
    let mut store = Store::default();
    store.learn("date_of_birth", "1985-03-12");
    let a = Assistant::new(store, None, None);
    let pages = vec![PageGeom::new([0.0, 0.0, 595.0, 842.0], 0)];
    let fields = vec![
        comb_row(100.0, 100.0, 4),
        comb_row(160.0, 100.0, 2),
        comb_row(196.0, 100.0, 2),
    ];
    let text = HashMap::from([(0, vec![label_at(20.0, 100.0, "Date of birth")])]);
    let an = a.analyse(&fields, &text, &pages);
    let got: Vec<String> = (0..3)
        .map(|i| a.confident(&an, &fields, i).unwrap())
        .collect();
    assert_eq!(
        got,
        vec!["1985", "03", "12"],
        "year first means year-month-day"
    );

    // Typing a phone number first still offers that person's name.
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut f, text, pages) = synthetic(&["Full name", "Phone"]);
    f[0].value = "Anna Berg".into();
    f[1].value = "0701234567".into();
    let an = a.analyse(&f, &text, &pages);
    a.learn(&an, &f, &[false; 2]);
    let (mut fresh, text, pages) = synthetic(&["Full name", "Phone"]);
    fresh[1].value = "0709999999".into();
    let an = a.analyse(&fresh, &text, &pages);
    assert_eq!(a.confident(&an, &fresh, 0).as_deref(), Some("Anna Berg"));
}

#[test]
fn forgetting_removes_value_as_typed_and_two_surnames_keep_order() {
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut f, text, pages) = synthetic(&["IBAN"]);
    f[0].value = "SE45 5000 0000 0583 9825 7466".into();
    let an = a.analyse(&f, &text, &pages);
    a.learn(&an, &f, &[false]);
    a.store.forget_value("iban", "SE4550000000058398257466");
    let (fresh, text, pages) = synthetic(&["IBAN"]);
    let an = a.analyse(&fresh, &text, &pages);
    assert_eq!(a.confident(&an, &fresh, 0), None, "forgotten everywhere");

    // Given name and two surnames typed separately: family-first is right.
    let mut a = Assistant::new(Store::default(), None, None);
    let (mut f, text, pages) = synthetic(&["Given name", "Family name"]);
    f[0].value = "Ana".into();
    f[1].value = "García López".into();
    let an = a.analyse(&f, &text, &pages);
    a.learn(&an, &f, &[false; 2]);
    let (fresh, text, pages) = synthetic(&["Apellidos y Nombre"]);
    let an = a.analyse(&fresh, &text, &pages);
    assert_eq!(
        a.confident(&an, &fresh, 0).as_deref(),
        Some("García López, Ana")
    );
}
