//! Forms made only for Adobe Reader (XFA).
//!
//! A "dynamic" XFA form keeps the form in an XML packet that only Adobe's
//! own apps lay out; the PDF pages hold just a "Please wait…" placeholder.
//! No other reader can show it (Okular's request for it has been open for
//! years), so the honest thing is to say so rather than show a blank page.
//! Forms that carry XFA *and* real AcroForm fields are filled as usual;
//! saving drops the XFA so other apps show what was typed.

use lopdf::{Document, Object};

/// Whether the file is an XFA form with nothing else to show: an XFA
/// packet, and either no AcroForm fields or a request to be laid out from
/// the XFA (`/NeedsRendering true`).
pub fn is_xfa_only(doc: &Document) -> bool {
    let Ok(catalog) = doc.catalog() else {
        return false;
    };
    let needs_rendering = matches!(catalog.get(b"NeedsRendering"), Ok(Object::Boolean(true)));
    let Some(form) = catalog.get(b"AcroForm").ok().and_then(|o| match o {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }) else {
        return false;
    };
    if form.get(b"XFA").is_err() {
        return false;
    }
    let fields = form
        .get(b"Fields")
        .ok()
        .and_then(|o| match o {
            Object::Reference(id) => doc.get_object(*id).ok(),
            o => Some(o),
        })
        .and_then(|o| o.as_array().ok())
        .map_or(0, |a| a.len());
    needs_rendering || fields == 0
}

#[cfg(test)]
mod tests {
    use super::is_xfa_only;
    use lopdf::{Document, Object, dictionary};

    fn doc(form: lopdf::Dictionary, needs_rendering: bool) -> Document {
        let mut d = Document::with_version("1.7");
        let form_id = d.add_object(form);
        let pages = d.add_object(dictionary! { "Type" => "Pages", "Kids" => vec![], "Count" => 0 });
        let mut cat = dictionary! { "Type" => "Catalog", "Pages" => pages, "AcroForm" => form_id };
        if needs_rendering {
            cat.set("NeedsRendering", Object::Boolean(true));
        }
        let cat_id = d.add_object(cat);
        d.trailer.set("Root", cat_id);
        d
    }

    #[test]
    fn xfa_without_fields_is_xfa_only() {
        let d = doc(
            dictionary! { "XFA" => Object::string_literal("<xdp/>"), "Fields" => vec![] },
            false,
        );
        assert!(is_xfa_only(&d));
    }

    #[test]
    fn xfa_asking_to_be_rendered_is_xfa_only() {
        let field = Object::Reference((99, 0));
        let d = doc(
            dictionary! { "XFA" => Object::string_literal("<xdp/>"), "Fields" => vec![field] },
            true,
        );
        assert!(is_xfa_only(&d));
    }

    #[test]
    fn xfa_with_real_fields_is_filled_as_usual() {
        let field = Object::Reference((99, 0));
        let d = doc(
            dictionary! { "XFA" => Object::string_literal("<xdp/>"), "Fields" => vec![field] },
            false,
        );
        assert!(!is_xfa_only(&d));
    }

    #[test]
    fn a_plain_form_is_not_xfa() {
        let d = doc(dictionary! { "Fields" => vec![] }, false);
        assert!(!is_xfa_only(&d));
    }
}
