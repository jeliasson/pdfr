//! Damaged or hostile files don't crash or hang: text sorted in a way a
//! tolerance-based comparison can't order, a page kilometres wide, a
//! stream that inflates to gigabytes, Form XObjects drawn from each other
//! many times over, absurd font sizes and checkbox states that aren't
//! UTF-8.

use lopdf::{Dictionary, Document, Object, Stream, StringFormat, dictionary};
use pdfr_core::forms::FormDoc;
use pdfr_core::geom::Rect;
use pdfr_core::redact::{Mark, redact};
use pdfr_core::text::TextRun;
use std::time::Instant;

fn rect(r: [i64; 4]) -> Object {
    Object::Array(r.iter().map(|&v| v.into()).collect())
}

fn s(t: &str) -> Object {
    Object::String(t.as_bytes().to_vec(), StringFormat::Literal)
}

fn save(mut d: Document) -> Vec<u8> {
    let mut out = Vec::new();
    d.save_to(&mut out).unwrap();
    out
}

/// A document of one page with `media` as its MediaBox, `content` and
/// what `extra` adds to the page dictionary and the catalog.
fn doc(
    media: [i64; 4],
    content: Vec<u8>,
    extra: impl FnOnce(&mut Document, lopdf::ObjectId, &mut Dictionary, &mut Dictionary),
) -> Vec<u8> {
    let mut d = Document::with_version("1.7");
    let font =
        d.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let c = d.add_object(Stream::new(dictionary! {}, content));
    let pages_id = d.new_object_id();
    let page_id = d.new_object_id();
    let mut page = dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>rect(media),"Contents"=>c,
        "Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},
    };
    let mut cat = dictionary! {"Type"=>"Catalog","Pages"=>pages_id};
    extra(&mut d, page_id, &mut page, &mut cat);
    d.objects.insert(page_id, Object::Dictionary(page));
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page_id.into()],"Count"=>1}),
    );
    let cat = d.add_object(cat);
    d.trailer.set("Root", cat);
    save(d)
}

// ---------------------------------------------------------------------------
// C1: reading order.

fn run(x: f32, y: f32, size: f32, text: &str) -> TextRun {
    TextRun {
        rect: Rect::new(x, y - size, x + 30.0, y),
        text: text.into(),
        size,
        xs: vec![],
        ws: vec![],
    }
}

#[test]
fn jittered_baselines_sort_without_panicking() {
    // Baselines that drift a little from run to run: "same line" chains
    // across lines, which a tolerance-based comparison can't order.
    let mut seed = 12345u64;
    let mut rnd = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f32) / (1u64 << 31) as f32
    };
    for _ in 0..20 {
        let runs: Vec<TextRun> = (0..500)
            .map(|i| {
                let y = 100.0 + (i / 20) as f32 * 3.0 + rnd() * 4.0;
                run(rnd() * 500.0, y, 4.0 + rnd() * 10.0, "x")
            })
            .collect();
        let order = pdfr_core::text::reading_order(&runs);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..500).collect::<Vec<_>>(), "a permutation");
        let mut items: Vec<(usize, Rect)> = runs.iter().map(|r| (0, r.rect)).collect();
        pdfr_core::detect::reading_order_by(&mut items, |i| *i);
        assert_eq!(items.len(), 500);
    }
}

#[test]
fn reading_order_of_a_plain_page_is_unchanged() {
    // Three lines, the middle one with a slightly lower second phrase.
    let runs = vec![
        run(200.0, 140.0, 10.0, "c2"),
        run(50.0, 100.0, 10.0, "a1"),
        run(200.0, 101.0, 10.0, "a2"),
        run(50.0, 140.0, 10.0, "c1"),
        run(50.0, 120.0, 10.0, "b1"),
        run(200.0, 121.5, 10.0, "b2"),
    ];
    let order: Vec<&str> = pdfr_core::text::reading_order(&runs)
        .into_iter()
        .map(|i| runs[i].text.as_str())
        .collect();
    assert_eq!(order, ["a1", "a2", "b1", "b2", "c1", "c2"]);
}

// ---------------------------------------------------------------------------
// C2: huge pages.

#[test]
fn huge_page_opens_renders_capped_and_skips_detection() {
    let bytes = doc(
        [0, 0, 2_000_000, 1_000],
        b"BT /F1 12 Tf 100 500 Td (Hi) Tj ET".to_vec(),
        |_, _, _, _| {},
    );
    let fd = FormDoc::open(bytes.clone()).unwrap();
    assert_eq!(fd.pages.len(), 1);
    let pdf = pdfr_core::render::open(bytes).unwrap();
    let pages = pdf.pages();
    let p = &pages[0];
    let s = pdfr_core::render::render_scale(p, 2.0);
    assert!((s - 8192.0 / 2_000_000.0).abs() < 1e-9, "{s}");
    assert!(pdfr_core::render::render_scale(p, f32::NAN) <= s);
    let cache = pdfr_core::render::RenderCache::new();
    let img = pdfr_core::render::render_page(p, &cache, 2.0);
    assert!(img.width as f32 <= pdfr_core::render::MAX_RENDER_SIDE);
    assert!(img.height as f32 <= pdfr_core::render::MAX_RENDER_SIDE);
    assert!(!pdfr_core::detect::detectable(p));
    let (fields, gray) = pdfr_core::detect::detect_page_gray(p, &cache, 0);
    assert!(fields.is_empty() && gray.px.is_empty());
}

// ---------------------------------------------------------------------------
// C3: decompression bombs.

/// A zlib stream that inflates to a little over `out` zero bytes, about
/// 1/1000 of that in size (one literal, then the longest copies).
fn zlib_bomb(out: usize) -> Vec<u8> {
    struct Bits {
        buf: Vec<u8>,
        acc: u64,
        n: u32,
    }
    impl Bits {
        fn put(&mut self, v: u64, n: u32) {
            self.acc |= v << self.n;
            self.n += n;
            while self.n >= 8 {
                self.buf.push(self.acc as u8);
                self.acc >>= 8;
                self.n -= 8;
            }
        }
        /// A Huffman code: most significant bit first.
        fn code(&mut self, v: u64, n: u32) {
            let r = (0..n).fold(0, |a, i| (a << 1) | ((v >> i) & 1));
            self.put(r, n);
        }
    }
    let mut b = Bits {
        buf: vec![0x78, 0x9c],
        acc: 0,
        n: 0,
    };
    b.put(1, 1); // last block
    b.put(1, 2); // fixed Huffman codes
    b.code(0x30, 8); // literal 0
    for _ in 0..out / 258 + 1 {
        b.code(0b1100_0101, 8); // length 258
        b.code(0, 5); // distance 1
    }
    b.code(0, 7); // end of block
    b.put(0, 7);
    b.buf.extend_from_slice(&[0, 0, 0, 0]); // (wrong) Adler-32
    b.buf
}

fn bomb_stream(dict: Dictionary) -> Stream {
    let mut dict = dict;
    dict.set("Filter", "FlateDecode");
    // About 200 MiB inflated, well over the 64 MiB limit.
    Stream::new(dict, zlib_bomb(200 << 20)).with_compression(false)
}

#[test]
fn bomb_in_page_content_is_refused_by_redaction() {
    let mut d = Document::with_version("1.7");
    let pages_id = d.new_object_id();
    let c = d.add_object(bomb_stream(dictionary! {}));
    let page = d.add_object(dictionary! {
        "Type"=>"Page","Parent"=>pages_id,"MediaBox"=>rect([0,0,612,792]),"Contents"=>c,
    });
    d.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Kids"=>vec![page.into()],"Count"=>1}),
    );
    let cat = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
    d.trailer.set("Root", cat);
    let bytes = save(d);
    assert!(bytes.len() < 2 << 20, "the file itself is small");
    let t = Instant::now();
    let r = redact(
        &bytes,
        &[Mark {
            page: 0,
            rect: [0.0, 0.0, 10.0, 10.0],
        }],
    );
    assert!(r.is_err(), "refused");
    assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
}

#[test]
fn bomb_in_a_form_under_the_box_is_blacked() {
    let bytes = doc(
        [0, 0, 612, 792],
        b"q 1 0 0 1 100 600 cm /Fm0 Do Q".to_vec(),
        |d, _, page, _| {
            let form = d.add_object(bomb_stream(
                dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,50,50])},
            ));
            let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
            res.set("XObject", dictionary! {"Fm0"=>form});
        },
    );
    let out = redact(
        &bytes,
        &[Mark {
            page: 0,
            rect: [90.0, 590.0, 200.0, 700.0],
        }],
    )
    .unwrap();
    assert_eq!(out.forms_blacked, 1);
    assert!(out.bytes.len() < 1 << 20, "the bomb is gone");
}

#[test]
fn bomb_as_a_field_appearance_saves() {
    let bytes = doc([0, 0, 612, 792], Vec::new(), |d, page_id, page, cat| {
        let ap = d.add_object(bomb_stream(
            dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,100,20])},
        ));
        let w = d.add_object(dictionary! {
            "Type"=>"Annot","Subtype"=>"Widget","FT"=>"Tx","T"=>s("name"),"V"=>s("old"),
            "DA"=>s("/Helv 0 Tf 0 g"),"Rect"=>rect([100,600,200,620]),"P"=>page_id,
            "AP"=>dictionary!{"N"=>ap},
        });
        page.set("Annots", vec![w.into()]);
        cat.set("AcroForm", dictionary! {"Fields"=>vec![w.into()]});
    });
    let fd = FormDoc::open(bytes).unwrap();
    let mut fields = fd.fields.clone();
    fields[0].value = "new".into();
    let t = Instant::now();
    let out = pdfr_core::save::save(&fd, &fields).unwrap();
    assert!(t.elapsed().as_secs() < 20, "{:?}", t.elapsed());
    let back = FormDoc::open(out).unwrap();
    assert_eq!(back.fields[0].value, "new");
}

#[test]
fn stripping_text_is_linear() {
    // Many `BT` with no `ET` after them.
    let content = b"BT ".repeat(200_000);
    let t = Instant::now();
    let out = pdfr_core::forms::strip_variable_text(&content);
    assert_eq!(out, content);
    assert!(t.elapsed().as_secs() < 2, "{:?}", t.elapsed());
}

// ---------------------------------------------------------------------------
// C4: tree walks in redaction.

#[test]
fn forms_drawn_from_forms_many_times_over_finish() {
    // Five levels, each drawing the one below ten times at different
    // places: 100,000 placements, each one rewritten if followed.
    let bytes = doc(
        [0, 0, 612, 792],
        b"q 1 0 0 1 100 600 cm /Fm0 Do Q".to_vec(),
        |d, _, page, _| {
            let font = page
                .get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"Font")
                .unwrap()
                .clone();
            let mut inner = d.add_object(Stream::new(
                dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,10,10]),
                "Resources"=>dictionary!{"Font"=>font}},
                b"BT /F1 2 Tf 0 2 Td (x) Tj ET".to_vec(),
            ));
            for _ in 0..5 {
                let ops: String = (0..10)
                    .map(|i| format!("q 1 0 0 1 {} 0 cm /Fm0 Do Q ", i))
                    .collect();
                inner = d.add_object(Stream::new(
                    dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>rect([0,0,20,10]),
                    "Resources"=>dictionary!{"XObject"=>dictionary!{"Fm0"=>inner}}},
                    ops.into_bytes(),
                ));
            }
            let res = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
            res.set("XObject", dictionary! {"Fm0"=>inner});
        },
    );
    // Drawing the page once (which redaction does too, to find images)
    // follows every placement; the rewrite itself must add little to that.
    let render = {
        let pdf = pdfr_core::render::open(bytes.clone()).unwrap();
        let pages = pdf.pages();
        let cache = pdfr_core::render::RenderCache::new();
        let t = Instant::now();
        pdfr_core::render::render_page(&pages[0], &cache, 0.1);
        t.elapsed()
    };
    let t = Instant::now();
    let out = redact(
        &bytes,
        &[Mark {
            page: 0,
            rect: [90.0, 590.0, 200.0, 700.0],
        }],
    );
    assert!(out.is_ok());
    let took = t.elapsed();
    assert!(
        took < render * 3 + std::time::Duration::from_secs(2),
        "redaction {took:?}, drawing {render:?}"
    );
}

#[test]
fn shared_field_tree_redacts_quickly() {
    // A field tree whose 20 levels each list the level below ten times.
    let bytes = doc([0, 0, 612, 792], Vec::new(), |d, page_id, page, cat| {
        let w = d.add_object(dictionary! {
            "Type"=>"Annot","Subtype"=>"Widget","FT"=>"Tx","T"=>s("w"),"V"=>s("SECRET"),
            "Rect"=>rect([100,600,200,620]),"P"=>page_id,
        });
        let mut node = d.add_object(dictionary! {"T"=>s("leaf"),"Kids"=>vec![w.into()]});
        for _ in 0..20 {
            node =
                d.add_object(dictionary! {"T"=>s("n"),"Kids"=>vec![Object::Reference(node); 10]});
        }
        page.set("Annots", vec![w.into()]);
        cat.set("AcroForm", dictionary! {"Fields"=>vec![node.into()]});
    });
    let t = Instant::now();
    let out = redact(
        &bytes,
        &[Mark {
            page: 0,
            rect: [90.0, 590.0, 250.0, 700.0],
        }],
    )
    .unwrap();
    assert!(t.elapsed().as_secs() < 5, "{:?}", t.elapsed());
    assert_eq!(out.annotations_removed, 1);
}
