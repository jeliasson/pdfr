//! Redaction and page extraction leave nothing behind: each test builds a
//! small PDF with a secret in a place the content rewriter does not draw
//! (an annotation, the structure tree, document details, a broken Form
//! XObject, a vertical font, a page left out), runs it through, and
//! searches every (decompressed) byte of the result.

use lopdf::{Dictionary, Document, Object, Stream, StringFormat, dictionary};
use pdfr_core::forms::FormDoc;
use pdfr_core::redact::{Mark, redact};

fn s(t: &str) -> Object {
    Object::String(t.as_bytes().to_vec(), StringFormat::Literal)
}

fn rect(r: [i64; 4]) -> Object {
    Object::Array(r.iter().map(|&v| v.into()).collect())
}

fn save(mut d: Document) -> Vec<u8> {
    let mut out = Vec::new();
    d.save_to(&mut out).unwrap();
    out
}

/// The file with every stream decompressed, as text.
fn plain(bytes: &[u8]) -> String {
    let mut d = Document::load_mem(bytes).unwrap();
    d.decompress();
    let mut out = Vec::new();
    d.save_to(&mut out).unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

fn contains(h: &[u8], n: &[u8]) -> bool {
    h.windows(n.len()).any(|w| w == n)
}

/// A one-page document: `content` drawn with Helvetica as `/F1`, plus
/// whatever `extra` adds (it gets the document, the page id and the page
/// dictionary before the page is stored).
fn one_page(
    content: &str,
    extra: impl FnOnce(&mut Document, lopdf::ObjectId, &mut Dictionary),
) -> Vec<u8> {
    let mut d = Document::with_version("1.7");
    let font =
        d.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let c = d.add_object(Stream::new(dictionary! {}, content.as_bytes().to_vec()));
    let pages_id = d.new_object_id();
    let page_id = d.new_object_id();
    let mut page = dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>rect([0,0,612,792]),"Contents"=>c,
        "Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},
    };
    extra(&mut d, page_id, &mut page);
    d.objects.insert(page_id, Object::Dictionary(page));
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page_id.into()],"Count"=>1}),
    );
    if d.trailer.get(b"Root").is_err() {
        let cat = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
        d.trailer.set("Root", cat);
    } else {
        let root = d.trailer.get(b"Root").unwrap().as_reference().unwrap();
        d.get_dictionary_mut(root).unwrap().set("Pages", pages_id);
    }
    save(d)
}

/// The box used throughout: covers x 90..250, y 390..720.
fn mark() -> Mark {
    Mark {
        page: 0,
        rect: [90.0, 390.0, 250.0, 720.0],
    }
}

fn redacted(bytes: &[u8]) -> String {
    plain(&redact(bytes, &[mark()]).unwrap().bytes)
}

fn assert_gone(txt: &str, secrets: &[&str]) {
    for k in secrets {
        assert!(!txt.contains(k), "{k} leaked");
    }
}

/// The redprobe document: a tagged widget whose appearance and value hold
/// secrets, a note with a reply outside the box, an annotation written
/// straight into `/Annots`, alternate text in the structure tree, a
/// bookmark and document details.
fn tagged() -> Vec<u8> {
    let mut d = Document::with_version("1.7");
    let font =
        d.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let content = d.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 100 700 Td (VISIBLETEXT) Tj ET BT /F1 12 Tf 300 100 Td (KEEPME) Tj ET"
            .to_vec(),
    ));
    let ap = d.add_object(Stream::new(
        dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,100,20])},
        b"/Tx BMC BT /F1 10 Tf 2 5 Td (APSECRET) Tj ET EMC".to_vec(),
    ));
    let pages_id = d.new_object_id();
    let page_id = d.new_object_id();
    let widget = d.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Widget","FT"=>"Tx","T"=>s("name"),"V"=>s("FIELDSECRET"),
        "Rect"=>rect([100,600,200,620]),"P"=>Object::Reference(page_id),
        "AP"=>dictionary!{"N"=>Object::Reference(ap)},"StructParent"=>0,
    });
    let note = d.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Text","Contents"=>s("NOTESECRET"),"Rect"=>rect([100,500,120,520]),
    });
    let reply = d.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Text","Contents"=>s("REPLYSECRET"),
        "IRT"=>Object::Reference(note),"Rect"=>rect([400,100,420,120]),
    });
    let inline = dictionary! {
        "Type"=>"Annot","Subtype"=>"FreeText","Contents"=>s("INLINESECRET"),"Rect"=>rect([100,400,200,420]),
    };
    let far = dictionary! {
        "Type"=>"Annot","Subtype"=>"FreeText","Contents"=>s("FARNOTE"),"Rect"=>rect([400,700,500,720]),
    };
    d.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>rect([0,0,612,792]),"Contents"=>content,
            "Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},
            "Annots"=>vec![widget.into(), note.into(), reply.into(), Object::Dictionary(inline), Object::Dictionary(far)],
            "StructParents"=>1,
        }),
    );
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page_id.into()],"Count"=>1}),
    );
    let objr = d.add_object(
        dictionary! {"Type"=>"OBJR","Obj"=>Object::Reference(widget),"Pg"=>Object::Reference(page_id)},
    );
    let se = d.add_object(dictionary! {
        "Type"=>"StructElem","S"=>"Form","K"=>vec![Object::Reference(objr)],"ActualText"=>s("STRUCTSECRET"),
    });
    let parent_tree = d.add_object(dictionary! {"Nums"=>vec![0.into(), Object::Reference(se)]});
    let str_root = d.add_object(dictionary! {
        "Type"=>"StructTreeRoot","K"=>vec![Object::Reference(se)],"ParentTree"=>parent_tree,
    });
    let form = d.add_object(dictionary! {"Fields"=>vec![widget.into()]});
    let ol_item = d.new_object_id();
    let ol = d.add_object(dictionary! {
        "Type"=>"Outlines","First"=>Object::Reference(ol_item),"Last"=>Object::Reference(ol_item),"Count"=>1,
    });
    d.objects.insert(
        ol_item,
        Object::Dictionary(dictionary! {"Title"=>s("OUTLINESECRET"),"Parent"=>ol}),
    );
    let xmp = d.add_object(Stream::new(
        dictionary! {"Type"=>"Metadata","Subtype"=>"XML"},
        b"<x:xmpmeta><dc:title>XMPSECRET</dc:title></x:xmpmeta>".to_vec(),
    ));
    let attach = d.add_object(Stream::new(
        dictionary! {"Type"=>"EmbeddedFile"},
        b"ATTACHSECRET".to_vec(),
    ));
    let spec = d.add_object(dictionary! {
        "Type"=>"Filespec","F"=>s("a.txt"),"EF"=>dictionary!{"F"=>attach},
    });
    let cat = d.add_object(dictionary! {
        "Type"=>"Catalog","Pages"=>pages_id,"AcroForm"=>form,"Outlines"=>ol,
        "StructTreeRoot"=>str_root,"MarkInfo"=>dictionary!{"Marked"=>true},"Metadata"=>xmp,
        "Names"=>dictionary!{"EmbeddedFiles"=>dictionary!{"Names"=>vec![s("a.txt"), spec.into()]}},
        "AF"=>vec![spec.into()],
    });
    let info = d.add_object(dictionary! {"Title"=>s("INFOSECRET"),"Author"=>s("AUTHORSECRET")});
    d.trailer.set("Root", cat);
    d.trailer.set("Info", info);
    save(d)
}

#[test]
fn tagged_widget_and_its_structure_are_gone() {
    let txt = redacted(&tagged());
    assert_gone(
        &txt,
        &["VISIBLETEXT", "APSECRET", "FIELDSECRET", "STRUCTSECRET"],
    );
    assert!(txt.contains("KEEPME"), "text outside the box stays");
}

#[test]
fn note_under_the_box_goes_with_its_replies() {
    let txt = redacted(&tagged());
    assert_gone(&txt, &["NOTESECRET", "REPLYSECRET"]);
}

#[test]
fn annotation_written_into_annots_is_removed() {
    let txt = redacted(&tagged());
    assert_gone(&txt, &["INLINESECRET"]);
    assert!(
        txt.contains("FARNOTE"),
        "annotations away from the box stay"
    );
}

#[test]
fn document_details_bookmarks_and_attachments_are_removed() {
    let out = redact(&tagged(), &[mark()]).unwrap().bytes;
    let txt = plain(&out);
    assert_gone(
        &txt,
        &[
            "INFOSECRET",
            "AUTHORSECRET",
            "OUTLINESECRET",
            "XMPSECRET",
            "ATTACHSECRET",
        ],
    );
    let d = Document::load_mem(&out).unwrap();
    let info = d
        .trailer
        .get(b"Info")
        .and_then(|o| d.dereference(o))
        .unwrap()
        .1
        .as_dict()
        .unwrap()
        .clone();
    assert_eq!(
        info.get(b"Producer").unwrap().as_str().unwrap(),
        b"pdfr",
        "Producer says pdfr"
    );
    let cat = d.catalog().unwrap();
    for k in [
        &b"StructTreeRoot"[..],
        b"MarkInfo",
        b"Metadata",
        b"Outlines",
        b"AF",
    ] {
        assert!(!cat.has(k), "{} removed", String::from_utf8_lossy(k));
    }
}

#[test]
fn named_alternate_text_of_removed_text_is_gone() {
    let bytes = one_page(
        "/Span /MC0 BDC BT /F1 12 Tf 100 600 Td (ALTVISIBLE) Tj ET EMC\n\
         /Span /MC1 BDC BT /F1 12 Tf 300 100 Td (OTHER) Tj ET EMC",
        |d, _, page| {
            let img_meta = d.add_object(Stream::new(
                dictionary! {"Type"=>"Metadata","Subtype"=>"XML"},
                b"IMGXMPSECRET".to_vec(),
            ));
            let img = d.add_object(Stream::new(
                dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,
                "ColorSpace"=>"DeviceGray","BitsPerComponent"=>8,"Metadata"=>img_meta},
                vec![128],
            ));
            let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
            res.set(
                "Properties",
                dictionary! {
                    "MC0"=>dictionary!{"ActualText"=>s("ALTSECRET")},
                    "MC1"=>dictionary!{"Alt"=>s("ALTKEPT")},
                },
            );
            res.set("XObject", dictionary! {"Im1"=>img});
        },
    );
    let txt = redacted(&bytes);
    assert_gone(&txt, &["ALTVISIBLE", "ALTSECRET", "IMGXMPSECRET"]);
    assert!(txt.contains("OTHER"));
}

/// A chain of `depth` Form XObjects, the last one writing `secret`, drawn
/// at (100, 600).
fn nested_forms(depth: usize, secret: &str) -> Vec<u8> {
    let secret = secret.to_string();
    one_page("q 1 0 0 1 100 600 cm /Fm0 Do Q", move |d, _, page| {
        let font = page
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .unwrap()
            .clone();
        let mut inner = d.add_object(Stream::new(
            dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,200,30]),
            "Resources"=>dictionary!{"Font"=>font}},
            format!("BT /F1 12 Tf 2 10 Td ({secret}) Tj ET").into_bytes(),
        ));
        for _ in 1..depth {
            inner = d.add_object(Stream::new(
                dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,200,30]),
                "Resources"=>dictionary!{"XObject"=>dictionary!{"Fm0"=>inner}}},
                b"/Fm0 Do".to_vec(),
            ));
        }
        let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
        res.set("XObject", dictionary! {"Fm0"=>inner});
    })
}

#[test]
fn nested_forms_are_followed_or_blacked() {
    // A few levels deep: rewritten as usual.
    let txt = redacted(&nested_forms(3, "SHALLOWSECRET"));
    assert_gone(&txt, &["SHALLOWSECRET"]);
    // Deeper than the rewriter follows: the form is replaced by black.
    let out = redact(&nested_forms(20, "DEEPSECRET"), &[mark()]).unwrap();
    assert_gone(&plain(&out.bytes), &["DEEPSECRET"]);
}

#[test]
fn undecodable_form_under_the_box_is_blacked() {
    let raw = b"\x00\x01BROKENSECRET\xff junk".to_vec();
    let bytes = one_page("q 1 0 0 1 100 600 cm /Fm0 Do Q", |d, _, page| {
        let form = d.add_object(Stream::new(
            dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,200,30]),
            "Filter"=>"JPXDecode"},
            raw.clone(),
        ));
        let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
        res.set("XObject", dictionary! {"Fm0"=>form});
    });
    let out = redact(&bytes, &[mark()]).unwrap().bytes;
    assert!(!contains(&out, b"BROKENSECRET"), "the broken form is gone");
    // In its place, a black rectangle over its bounding box.
    let d = Document::load_mem(&out).unwrap();
    let page = *d.get_pages().values().next().unwrap();
    let ops = lopdf::content::Content::decode(&d.get_page_content(page))
        .unwrap()
        .operations;
    let bbox = ops.iter().any(|o| {
        o.operator == "re"
            && o.operands
                .iter()
                .map(|v| v.as_float().unwrap())
                .eq([0.0, 0.0, 200.0, 30.0])
    });
    assert!(bbox, "black over the form's bounds: {ops:?}");
}

#[test]
fn vertical_font_string_under_the_box_is_removed() {
    // Identity-V: 2-byte codes, written top to bottom from (100, 710), so
    // the string runs down into the box although it starts beside it.
    let bytes = one_page(
        "BT /F2 20 Tf 1 0 0 1 95 760 Tm (VERTSECRETXY) Tj ET",
        |d, _, page| {
            let cid = d.add_object(dictionary! {
                "Type"=>"Font","Subtype"=>"CIDFontType2","BaseFont"=>"Foo",
                "CIDSystemInfo"=>dictionary!{"Registry"=>s("Adobe"),"Ordering"=>s("Identity"),"Supplement"=>0},
                "FontDescriptor"=>dictionary!{"Type"=>"FontDescriptor","FontName"=>"Foo","Flags"=>4,
                    "FontBBox"=>rect([0,-200,1000,800]),"ItalicAngle"=>0,"Ascent"=>800,"Descent"=>-200,
                    "CapHeight"=>700,"StemV"=>80},
            });
            let f2 = d.add_object(dictionary! {
                "Type"=>"Font","Subtype"=>"Type0","BaseFont"=>"Foo","Encoding"=>"Identity-V",
                "DescendantFonts"=>vec![cid.into()],
            });
            let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
            let fonts = res.get_mut(b"Font").unwrap().as_dict_mut().unwrap();
            fonts.set("F2", f2);
        },
    );
    let txt = redacted(&bytes);
    assert_gone(&txt, &["VERTSECRETXY", "VERT", "SECRET"]);
}

#[test]
fn inline_font_string_touching_the_box_is_removed() {
    // A font written straight into the resources: widths unknown.
    let bytes = one_page(
        "BT /F3 12 Tf 60 600 Td (INLINEFONTSECRET) Tj ET",
        |_, _, page| {
            let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
            let fonts = res.get_mut(b"Font").unwrap().as_dict_mut().unwrap();
            fonts.set(
                "F3",
                dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Courier"},
            );
        },
    );
    let txt = redacted(&bytes);
    assert_gone(&txt, &["INLINEFONTSECRET"]);
}

#[test]
fn malformed_quote_operator_does_not_panic() {
    let bytes = one_page("BT /F1 12 Tf 100 700 Td (abc) \" ET", |_, _, _| {});
    let r = std::panic::catch_unwind(|| redact(&bytes, &[mark()]));
    assert!(r.is_ok(), "no panic");
    assert!(r.unwrap().is_ok());
}

// ---------------------------------------------------------------------------
// Page extraction and deletion.

/// Two pages; page 1 links to page 2 directly, through a named
/// destination (both kinds) and through a bookmark; page 2 holds the secret.
fn two_pages() -> Vec<u8> {
    let mut d = Document::with_version("1.7");
    let pages_id = d.new_object_id();
    let c1 = d.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 100 700 Td (PAGEONE) Tj ET".to_vec(),
    ));
    let c2 = d.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 100 700 Td (PAGETWOSECRET) Tj ET".to_vec(),
    ));
    let p2 = d.add_object(dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>rect([0,0,612,792]),"Contents"=>c2,"Resources"=>dictionary!{},
    });
    let p2dest = || Object::Array(vec![Object::Reference(p2), "Fit".into()]);
    let link = d.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Link","Rect"=>rect([0,0,50,50]),"Dest"=>p2dest(),
    });
    let link2 = d.add_object(dictionary! {
        "Type"=>"Annot","Subtype"=>"Link","Rect"=>rect([0,60,50,110]),
        "A"=>dictionary!{"S"=>"GoTo","D"=>p2dest()},
    });
    let p1 = d.add_object(dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>rect([0,0,612,792]),"Contents"=>c1,
        "Resources"=>dictionary!{},"Annots"=>vec![link.into(), link2.into()],
    });
    let p1dest = Object::Array(vec![Object::Reference(p1), "Fit".into()]);
    d.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages","Kids"=>vec![p1.into(),p2.into()],"Count"=>2},
        ),
    );
    let names = d.add_object(dictionary! {
        "Names"=>vec![s("one"), p1dest.clone(), s("two"), p2dest()],
    });
    let ol_item = d.new_object_id();
    let ol = d.add_object(dictionary! {
        "Type"=>"Outlines","First"=>Object::Reference(ol_item),"Last"=>Object::Reference(ol_item),"Count"=>1,
    });
    d.objects.insert(
        ol_item,
        Object::Dictionary(dictionary! {"Title"=>s("Two"),"Parent"=>ol,"Dest"=>p2dest()}),
    );
    let cat = d.add_object(dictionary! {
        "Type"=>"Catalog","Pages"=>pages_id,
        "Dests"=>dictionary!{"old"=>p2dest(),"first"=>p1dest},
        "Names"=>dictionary!{"Dests"=>names},
        "Outlines"=>ol,
        "OpenAction"=>p2dest(),
    });
    d.trailer.set("Root", cat);
    save(d)
}

#[test]
fn extracted_page_leaves_the_other_behind() {
    let fd = FormDoc::open(two_pages()).unwrap();
    let out = pdfr_core::pages::extract(&fd, &fd.fields, &fd.markups, &[0]).unwrap();
    let o = Document::load_mem(&out).unwrap();
    assert_eq!(o.get_pages().len(), 1);
    let txt = plain(&out);
    assert!(txt.contains("PAGEONE"));
    assert_gone(&txt, &["PAGETWOSECRET"]);
}

#[test]
fn deleted_page_is_gone_and_links_to_it_too() {
    let fd = FormDoc::open(two_pages()).unwrap();
    let kept = fd.arranged(&[0]).unwrap();
    let txt = plain(&kept.bytes);
    assert!(txt.contains("PAGEONE"));
    assert_gone(&txt, &["PAGETWOSECRET"]);
    let d = Document::load_mem(&kept.bytes).unwrap();
    let cat = d.catalog().unwrap();
    // The named destination to page 1 stays, the one to page 2 goes.
    let dests = cat.get(b"Dests").unwrap();
    let dests = d.dereference(dests).unwrap().1.as_dict().unwrap();
    assert!(dests.has(b"first") && !dests.has(b"old"));
    let names = cat.get(b"Names").unwrap().as_dict().unwrap();
    let tree = d.dereference(names.get(b"Dests").unwrap()).unwrap().1;
    let arr = tree.as_dict().unwrap().get(b"Names").unwrap();
    let arr = d.dereference(arr).unwrap().1.as_array().unwrap();
    assert_eq!(arr.len(), 2, "only page 1's name is left");
    assert!(!cat.has(b"OpenAction"), "the open action pointed at page 2");
    let outline = d.dereference(cat.get(b"Outlines").unwrap()).unwrap().1;
    assert!(
        !outline.as_dict().unwrap().has(b"First"),
        "the bookmark to page 2 is gone"
    );
    // The links on page 1 that went to page 2 are gone.
    let page = *d.get_pages().values().next().unwrap();
    let annots = d.get_dictionary(page).unwrap().get(b"Annots");
    let n = annots
        .ok()
        .and_then(|a| d.dereference(a).ok())
        .and_then(|(_, a)| a.as_array().ok().map(|a| a.len()))
        .unwrap_or(0);
    assert_eq!(n, 0);
}
