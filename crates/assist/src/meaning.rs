//! Label → meaning: normalisation and the built-in dictionary.

use serde::{Deserialize, Serialize};

/// Canonical keys known out of the box. New keys (`new:<snake_case>` from the
/// AI) are added to the vocabulary at run time.
pub const BASE_KEYS: &[&str] = &[
    "full_name",
    "given_name",
    "family_name",
    "middle_name",
    "email",
    "phone",
    "street_address",
    "postal_code",
    "city",
    "region",
    "country",
    "nationality",
    "date_of_birth",
    "place_of_birth",
    "gender",
    "national_id",
    "passport_number",
    "tax_id",
    "iban",
    "bic",
    "bank_name",
    "company_name",
    "company_id",
    "job_title",
    "signature_date",
    "signature_place",
    "website",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum Part {
    Day,
    Month,
    Year,
}

/// A canonical key with optional part / format hints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Meaning {
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<Part>,
    /// For names: the family name comes first ("Apellidos y Nombre").
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub family_first: bool,
}

impl Meaning {
    pub fn new(key: &str) -> Self {
        Meaning {
            key: key.into(),
            part: None,
            family_first: false,
        }
    }
    pub fn is_date(&self) -> bool {
        // Whole words of the key: `date_of_birth`, `expiry_date`, not `candidate_id`.
        self.key.split('_').any(|w| w == "date" || w == "dob")
    }
}

fn strip_accent(c: char) -> char {
    match c {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ę' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ø' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ñ' => 'n',
        // Spanish ordinal letters: "Dª" (doña), "nº" (número).
        'ª' => 'a',
        'º' => 'o',
        'ç' => 'c',
        'ý' | 'ÿ' => 'y',
        'ß' => 's',
        'æ' => 'a',
        'œ' => 'o',
        'ł' => 'l',
        'š' => 's',
        'ž' => 'z',
        'č' => 'c',
        c => c,
    }
}

/// Lowercase, strip accents, punctuation, numbering ("1.", "(a)") and
/// trailing colons, collapse spaces.
pub fn normalise(label: &str) -> String {
    let lower: String = label.to_lowercase().chars().map(strip_accent).collect();
    // Dotted initials are one word: "D.N.I." -> "dni", "N.I.E." -> "nie".
    let lower: String = lower
        .split_whitespace()
        .map(|w| {
            let letters: Vec<&str> = w.split('.').filter(|p| !p.is_empty()).collect();
            if letters.len() >= 2
                && letters
                    .iter()
                    .all(|p| p.chars().count() == 1 && p.chars().all(char::is_alphanumeric))
            {
                letters.concat()
            } else {
                w.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut words: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect();
    // Leading numbering: "1", "1a", "a" (from "(a)"), "iv".
    while let Some(first) = words.first() {
        let numbering = first.chars().all(|c| c.is_ascii_digit())
            || (first.len() <= 3 && first.chars().next().is_some_and(|c| c.is_ascii_digit()))
            || (words.len() > 1 && first.len() == 1 && label.trim_start().starts_with('('))
            || (words.len() > 1
                && ["i", "ii", "iii", "iv", "v", "vi"].contains(&first.as_str())
                && label.trim_start().chars().nth(first.len()) == Some('.'));
        if numbering && words.len() > 1 {
            words.remove(0);
        } else {
            break;
        }
    }
    words.join(" ")
}

/// Hints about the expected date order, from a label like "(DD MM YYYY)".
pub fn date_order(label: &str) -> Option<Vec<Part>> {
    let n = label.to_lowercase();
    let mut found: Vec<(usize, Part)> = Vec::new();
    let find = |pats: &[&str]| pats.iter().filter_map(|p| n.find(p)).min();
    if let Some(i) = find(&["dd", "jj", "tt"]) {
        found.push((i, Part::Day));
    }
    if let Some(i) = find(&["mm"]) {
        found.push((i, Part::Month));
    }
    if let Some(i) = find(&["yyyy", "aaaa", "åååå", "jjjj", "yy", "aa", "åå"]) {
        found.push((i, Part::Year));
    }
    if found.len() < 2 {
        return None;
    }
    found.sort_by_key(|f| f.0);
    Some(found.into_iter().map(|f| f.1).collect())
}

/// `(normalised phrase, key)`: exact phrases and phrase fragments.
/// Matching prefers an exact phrase, then the longest fragment contained
/// in the label as whole words.
const DICT: &[(&str, &str)] = &[
    // --- English
    ("full name", "full_name"),
    ("name", "full_name"),
    ("your name", "full_name"),
    ("applicant name", "full_name"),
    ("name of applicant", "full_name"),
    ("name and surname", "full_name"),
    ("first name and surname", "full_name"),
    ("firstname surname", "full_name"),
    ("first name last name", "full_name"),
    ("name in full", "full_name"),
    ("printed name", "full_name"),
    ("print name", "full_name"),
    ("first name", "given_name"),
    ("firstname", "given_name"),
    ("given name", "given_name"),
    ("given names", "given_name"),
    ("forename", "given_name"),
    ("forenames", "given_name"),
    ("middle name", "middle_name"),
    ("last name", "family_name"),
    ("lastname", "family_name"),
    ("surname", "family_name"),
    ("family name", "family_name"),
    ("email", "email"),
    ("e mail", "email"),
    ("email address", "email"),
    ("e mail address", "email"),
    ("phone", "phone"),
    ("telephone", "phone"),
    ("phone number", "phone"),
    ("telephone number", "phone"),
    ("mobile", "phone"),
    ("mobile phone", "phone"),
    ("mobile number", "phone"),
    ("tel", "phone"),
    ("address", "street_address"),
    ("street address", "street_address"),
    ("street", "street_address"),
    ("home address", "street_address"),
    ("address line 1", "street_address"),
    ("postal address", "street_address"),
    ("postcode", "postal_code"),
    ("post code", "postal_code"),
    ("postal code", "postal_code"),
    ("zip", "postal_code"),
    ("zip code", "postal_code"),
    ("city", "city"),
    ("town", "city"),
    ("town city", "city"),
    ("city town", "city"),
    ("state", "region"),
    ("county", "region"),
    ("province", "region"),
    ("country", "country"),
    ("country of residence", "country"),
    ("nationality", "nationality"),
    ("citizenship", "nationality"),
    ("date of birth", "date_of_birth"),
    ("birth date", "date_of_birth"),
    ("birthdate", "date_of_birth"),
    ("dob", "date_of_birth"),
    ("place of birth", "place_of_birth"),
    ("gender", "gender"),
    ("sex", "gender"),
    ("personal number", "national_id"),
    ("personal identity number", "national_id"),
    ("national id", "national_id"),
    ("national identity number", "national_id"),
    ("social security number", "national_id"),
    ("ssn", "national_id"),
    ("id number", "national_id"),
    ("passport number", "passport_number"),
    ("passport no", "passport_number"),
    ("tax id", "tax_id"),
    ("tax number", "tax_id"),
    ("tax identification number", "tax_id"),
    ("tin", "tax_id"),
    ("vat number", "tax_id"),
    ("iban", "iban"),
    ("account number iban", "iban"),
    ("bic", "bic"),
    ("swift", "bic"),
    ("bic swift", "bic"),
    ("bank", "bank_name"),
    ("bank name", "bank_name"),
    ("company", "company_name"),
    ("company name", "company_name"),
    ("employer", "company_name"),
    ("organisation", "company_name"),
    ("organization", "company_name"),
    ("company registration number", "company_id"),
    ("occupation", "job_title"),
    ("job title", "job_title"),
    ("profession", "job_title"),
    ("position", "job_title"),
    ("date", "signature_date"),
    ("signature date", "signature_date"),
    ("date signed", "signature_date"),
    ("today s date", "signature_date"),
    ("place", "signature_place"),
    ("place and date", "signature_place"),
    ("website", "website"),
    // Not personal / not reusable.
    ("signature", "none"),
    ("sign here", "none"),
    ("notes", "none"),
    ("comments", "none"),
    ("remarks", "none"),
    ("description", "none"),
    ("quantity", "none"),
    ("qty", "none"),
    ("unit price", "none"),
    ("price", "none"),
    ("total", "none"),
    ("amount", "none"),
    // --- Spanish
    ("nombre", "given_name"),
    ("nombre completo", "full_name"),
    ("nombre y apellidos", "full_name"),
    ("apellidos y nombre", "full_name"),
    ("apellidos nombre", "full_name"),
    ("nombre apellidos", "full_name"),
    ("apellido", "family_name"),
    ("apellidos", "family_name"),
    ("primer apellido", "family_name"),
    ("segundo apellido", "none"),
    ("correo", "email"),
    ("correo electronico", "email"),
    ("email correo electronico", "email"),
    ("telefono", "phone"),
    ("movil", "phone"),
    ("telefono movil", "phone"),
    ("domicilio", "street_address"),
    ("domicilio a efectos de notificacion", "street_address"),
    ("domicilio a efectos de notificaciones", "street_address"),
    ("domicilio para notificaciones", "street_address"),
    // "D./Dª", "D/Dña.", "Don/Doña", "Sr./Sra.": the person's full name.
    ("d da", "full_name"),
    ("d dna", "full_name"),
    ("d dona", "full_name"),
    ("don dona", "full_name"),
    ("sr sra", "full_name"),
    ("numero de entrada", "none"),
    ("numero de registro", "none"),
    ("numero de expediente", "none"),
    ("en el dia", "none"),
    ("direccion", "street_address"),
    ("calle", "street_address"),
    ("codigo postal", "postal_code"),
    ("cp", "postal_code"),
    ("localidad", "city"),
    ("municipio", "city"),
    ("ciudad", "city"),
    ("poblacion", "city"),
    ("provincia", "region"),
    ("pais", "country"),
    ("nacionalidad", "nationality"),
    ("fecha de nacimiento", "date_of_birth"),
    ("lugar de nacimiento", "place_of_birth"),
    ("sexo", "gender"),
    ("dni", "national_id"),
    ("nie", "national_id"),
    ("dni nie", "national_id"),
    ("nif", "tax_id"),
    ("cif", "company_id"),
    ("numero de pasaporte", "passport_number"),
    ("pasaporte", "passport_number"),
    ("empresa", "company_name"),
    ("razon social", "company_name"),
    ("profesion", "job_title"),
    ("fecha", "signature_date"),
    ("firma", "none"),
    ("lugar", "signature_place"),
    ("observaciones", "none"),
    // --- Swedish
    ("namn", "full_name"),
    ("fullstandigt namn", "full_name"),
    ("namnfortydligande", "full_name"),
    ("fornamn", "given_name"),
    ("tilltalsnamn", "given_name"),
    ("efternamn", "family_name"),
    ("e post", "email"),
    ("epost", "email"),
    ("e postadress", "email"),
    ("mejl", "email"),
    ("telefon", "phone"),
    ("telefonnummer", "phone"),
    ("mobilnummer", "phone"),
    ("mobil", "phone"),
    ("adress", "street_address"),
    ("gatuadress", "street_address"),
    ("utdelningsadress", "street_address"),
    ("postnummer", "postal_code"),
    ("ort", "city"),
    ("postort", "city"),
    ("stad", "city"),
    ("land", "country"),
    ("medborgarskap", "nationality"),
    ("fodelsedatum", "date_of_birth"),
    ("fodelseort", "place_of_birth"),
    ("kon", "gender"),
    ("personnummer", "national_id"),
    ("samordningsnummer", "national_id"),
    ("passnummer", "passport_number"),
    ("organisationsnummer", "company_id"),
    ("foretag", "company_name"),
    ("foretagsnamn", "company_name"),
    ("arbetsgivare", "company_name"),
    ("yrke", "job_title"),
    ("titel", "job_title"),
    ("datum", "signature_date"),
    ("ort och datum", "signature_place"),
    ("underskrift", "none"),
    ("namnteckning", "none"),
    ("bank", "bank_name"),
    ("kontonummer", "none"),
    ("ovrigt", "none"),
    // --- German / French (common)
    ("vorname", "given_name"),
    ("nachname", "family_name"),
    ("geburtsdatum", "date_of_birth"),
    ("strasse", "street_address"),
    ("postleitzahl", "postal_code"),
    ("plz", "postal_code"),
    ("prenom", "given_name"),
    ("nom", "family_name"),
    ("nom de famille", "family_name"),
    ("date de naissance", "date_of_birth"),
    ("adresse", "street_address"),
    ("code postal", "postal_code"),
    ("ville", "city"),
    ("pays", "country"),
];

/// Phrases that mark a family-name-first name field.
const FAMILY_FIRST: &[&str] = &[
    "apellidos y nombre",
    "apellidos nombre",
    "apellido y nombre",
    "surname and first name",
    "surname first name",
    "last name first name",
    "family name given name",
    "surname and forename",
    "efternamn och fornamn",
    "efternamn fornamn",
    "nom prenom",
    "nachname vorname",
];

/// Words that make a name label obviously about someone else (guardian,
/// emergency contact, …). Those are left to the AI or to `none`.
const OTHER_PERSON: &[&str] = &[
    "mother",
    "father",
    "spouse",
    "partner",
    "child",
    "guardian",
    "emergency",
    "witness",
    "padre",
    "madre",
    "conyuge",
    "hijo",
    "mor",
    "far",
    "make",
    "maka",
    "vardnadshavare",
];

/// Words a date box's label may consist of ("Día", "DD MM AAAA", "Day of"),
/// so "Application day" is not taken for part of a birth date.
const DATE_WORDS: &[&str] = &[
    "day", "dia", "dag", "dd", "month", "mes", "manad", "mm", "year", "ano", "ar", "yyyy", "aaaa",
    "yy", "aa", "of", "de", "del", "y", "and", "och",
];

/// Drops connecting words at the ends of a normalised label: "con dni no"
/// -> "dni", "domicilio a efectos de notificacion en" -> "domicilio a
/// efectos de notificacion". Keeps the label if nothing would remain.
fn trim_connectors(n: &str) -> String {
    const LEADING: &[&str] = &[
        "con", "en", "y", "e", "a", "al", "de", "del", "el", "la", "los", "las", "su", "sus",
        "with", "and", "of", "the", "your",
    ];
    // "no" is "nº" (numero) after accent stripping.
    // Not "a": it is the feminine ending in "alumno/a", "interesado/a".
    const TRAILING: &[&str] = &[
        "en", "de", "del", "el", "la", "n", "no", "num", "numero", "nr", "nro",
    ];
    let mut w: Vec<&str> = n.split(' ').collect();
    while w.len() > 1 && LEADING.contains(&w[0]) {
        w.remove(0);
    }
    while w.len() > 1 && TRAILING.contains(w.last().unwrap()) {
        w.pop();
    }
    w.join(" ")
}

/// Resolves a label with the built-in dictionary. Returns `Some(None)` for a
/// known non-reusable label, `None` if unknown.
pub fn dictionary(label: &str) -> Option<Option<Meaning>> {
    let full = normalise(label);
    if full.is_empty() {
        return Some(None);
    }
    // Labels inside a sentence ("... con D.N.I. nº ____", "domicilio ... en
    // ____") carry connecting words at their ends; they say nothing. An exact
    // dictionary entry wins ("en el dia" is known as it is).
    let n = if DICT.iter().any(|(p, _)| *p == full) {
        full.clone()
    } else {
        trim_connectors(&full)
    };
    let words: Vec<&str> = n.split(' ').collect();
    if words.iter().any(|w| OTHER_PERSON.contains(w)) {
        return None;
    }
    let family_first = FAMILY_FIRST.iter().any(|p| contains_phrase(&n, p));
    let mut part = None;
    let mut key: Option<&str> = DICT.iter().find(|(p, _)| *p == n).map(|(_, k)| *k);
    if key.is_none() {
        // Longest fragment contained as whole words.
        key = DICT
            .iter()
            .filter(|(p, _)| p.len() >= 3 && contains_phrase(&n, p))
            .max_by_key(|(p, _)| p.len())
            .map(|(_, k)| *k);
        // Guard: the fragment must be at least half of the label's words; a
        // short generic word ("name", "nombre") inside a longer label is not
        // enough evidence and is left to the cache or the AI.
        if let Some(k) = key {
            let frag_words = DICT
                .iter()
                .filter(|(p, kk)| *kk == k && contains_phrase(&n, p))
                .map(|(p, _)| p.split(' ').count())
                .max()
                .unwrap_or(0);
            if frag_words * 2 < words.len() {
                key = None;
            }
        }
    }
    // Date parts: "Day", "Month", "Year", "Día", "Mes", "Año", "Dag", "Månad", "År".
    for (w, p) in [
        ("day", Part::Day),
        ("dia", Part::Day),
        ("dag", Part::Day),
        ("dd", Part::Day),
        ("month", Part::Month),
        ("mes", Part::Month),
        ("manad", Part::Month),
        ("mm", Part::Month),
        ("year", Part::Year),
        ("ano", Part::Year),
        ("ar", Part::Year),
        ("yyyy", Part::Year),
        ("aaaa", Part::Year),
    ] {
        if words.len() <= 4
            && words.contains(&w)
            && !words.is_empty()
            && (key.is_none() || words.len() == 1)
            && (key.is_some() || words.iter().all(|x| DATE_WORDS.contains(x)))
        {
            part = Some(p);
        }
    }
    if part.is_some() && key.is_none() {
        key = Some("date_of_birth");
    }
    let key = key?;
    if key == "none" {
        return Some(None);
    }
    let mut m = Meaning::new(key);
    m.family_first = family_first && key == "full_name";
    m.part = part;
    Some(Some(m))
}

fn contains_phrase(hay: &str, phrase: &str) -> bool {
    let h: Vec<&str> = hay.split(' ').collect();
    let p: Vec<&str> = phrase.split(' ').collect();
    h.windows(p.len()).any(|w| w == p.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(l: &str) -> Option<String> {
        dictionary(l).flatten().map(|m| m.key)
    }

    #[test]
    fn normalisation() {
        assert_eq!(normalise("  1. Full Name: "), "full name");
        assert_eq!(normalise("(a) Fecha de Nacimiento"), "fecha de nacimiento");
        assert_eq!(normalise("Apellidos y Nombre"), "apellidos y nombre");
        assert_eq!(normalise("E-mail address:"), "e mail address");
        assert_eq!(normalise("Födelsedatum"), "fodelsedatum");
    }

    #[test]
    fn three_languages_one_name() {
        assert_eq!(key("Firstname + surname").as_deref(), Some("full_name"));
        assert_eq!(key("Full name").as_deref(), Some("full_name"));
        assert_eq!(key("Apellidos y Nombre").as_deref(), Some("full_name"));
        assert!(
            dictionary("Apellidos y Nombre")
                .flatten()
                .unwrap()
                .family_first
        );
        assert!(!dictionary("Full name").flatten().unwrap().family_first);
        assert_eq!(key("Namn").as_deref(), Some("full_name"));
    }

    #[test]
    fn everyday_fields() {
        for (l, k) in [
            ("E-mail", "email"),
            ("Correo electrónico", "email"),
            ("E-postadress", "email"),
            ("Postcode", "postal_code"),
            ("Código postal", "postal_code"),
            ("Postnummer", "postal_code"),
            ("Date of birth (DD MM YYYY)", "date_of_birth"),
            ("Fecha de nacimiento", "date_of_birth"),
            ("Personnummer", "national_id"),
            ("DNI/NIE", "national_id"),
            ("IBAN", "iban"),
            ("Street address", "street_address"),
            ("City", "city"),
            ("Ort", "city"),
            ("Occupation", "job_title"),
            ("Date", "signature_date"),
        ] {
            assert_eq!(key(l).as_deref(), Some(k), "{l}");
        }
        assert_eq!(dictionary("Signature"), Some(None));
        assert_eq!(dictionary("Notes"), Some(None));
        assert_eq!(dictionary("Mother's maiden name"), None);
        assert_eq!(
            dictionary("Favourite colour of the applicant's first car"),
            None
        );
        assert_eq!(dictionary("Your pet's name"), None);
        assert_eq!(dictionary("Nombre del titular"), None);
        assert_eq!(key("Full name of applicant").as_deref(), Some("full_name"));
        assert_eq!(key("Postal code / ZIP").as_deref(), Some("postal_code"));
        assert_eq!(key("Telephone (mobile)").as_deref(), Some("phone"));
    }

    #[test]
    fn date_orders() {
        assert_eq!(
            date_order("Date of birth (DD MM YYYY)"),
            Some(vec![Part::Day, Part::Month, Part::Year])
        );
        assert_eq!(
            date_order("Birth date (mm/dd/yyyy)"),
            Some(vec![Part::Month, Part::Day, Part::Year])
        );
        assert_eq!(
            date_order("Födelsedatum (ÅÅÅÅ-MM-DD)"),
            Some(vec![Part::Year, Part::Month, Part::Day])
        );
        assert_eq!(date_order("Date"), None);
    }
}

#[cfg(test)]
mod sentence_labels {
    use super::*;

    fn key(label: &str) -> Option<String> {
        dictionary(label).flatten().map(|m| m.key)
    }

    #[test]
    fn spanish_sentence_form_labels() {
        assert_eq!(normalise("con D.N.I. nº"), "con dni no");
        assert_eq!(key("con D.N.I. nº").as_deref(), Some("national_id"));
        assert_eq!(key("D./Dª").as_deref(), Some("full_name"));
        assert_eq!(key("D/Dña.").as_deref(), Some("full_name"));
        assert_eq!(
            key("domicilio a efectos de notificación en").as_deref(),
            Some("street_address")
        );
        assert_eq!(
            dictionary("con número de entrada en este Ayuntamiento"),
            Some(None),
            "a form-specific number is not reusable"
        );
        assert_eq!(
            dictionary("en el día"),
            Some(None),
            "the application date is not reusable"
        );
        // Plain labels are unaffected.
        assert_eq!(key("Nombre").as_deref(), Some("given_name"));
        assert_eq!(key("Teléfono nº").as_deref(), Some("phone"));
    }
}
