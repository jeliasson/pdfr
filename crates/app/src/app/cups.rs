//! Printing where the desktop has no print portal (bare i3, Sway, dwm):
//! straight to the local CUPS server over IPP, the protocol CUPS speaks.
//! Only the standard library: a Unix socket (or localhost:631), one HTTP
//! request, and the few IPP attributes needed to list printers and send a
//! PDF (RFC 8010/8011).

use std::io::{Read, Write};
use std::time::Duration;

const SOCKETS: &[&str] = &["/run/cups/cups.sock", "/var/run/cups/cups.sock"];

// IPP value tags.
const T_INTEGER: u8 = 0x21;
const T_NAME: u8 = 0x42;
const T_KEYWORD: u8 = 0x44;
const T_URI: u8 = 0x45;
const T_CHARSET: u8 = 0x47;
const T_LANGUAGE: u8 = 0x48;
const T_MIME: u8 = 0x49;
// Group tags.
const G_OPERATION: u8 = 0x01;
const G_JOB: u8 = 0x02;
const G_END: u8 = 0x03;
const G_PRINTER: u8 = 0x04;

#[derive(Clone, Debug, PartialEq)]
pub struct Printer {
    pub name: String,
    /// The description shown to people ("Office printer").
    pub info: String,
}

impl Printer {
    pub fn label(&self) -> &str {
        if self.info.is_empty() {
            &self.name
        } else {
            &self.info
        }
    }
}

struct Request {
    buf: Vec<u8>,
}

impl Request {
    fn new(op: u16) -> Request {
        let mut buf = vec![1, 1];
        buf.extend(op.to_be_bytes());
        buf.extend(1u32.to_be_bytes());
        buf.push(G_OPERATION);
        let mut r = Request { buf };
        r.attr(T_CHARSET, "attributes-charset", b"utf-8");
        r.attr(T_LANGUAGE, "attributes-natural-language", b"en");
        r
    }

    fn attr(&mut self, tag: u8, name: &str, value: &[u8]) -> &mut Self {
        self.buf.push(tag);
        self.buf.extend((name.len() as u16).to_be_bytes());
        self.buf.extend(name.as_bytes());
        self.buf.extend((value.len() as u16).to_be_bytes());
        self.buf.extend(value);
        self
    }

    /// Another value of the previous attribute.
    fn more(&mut self, tag: u8, value: &[u8]) -> &mut Self {
        self.attr(tag, "", value)
    }

    fn group(&mut self, tag: u8) -> &mut Self {
        self.buf.push(tag);
        self
    }

    fn finish(mut self, data: &[u8]) -> Vec<u8> {
        self.buf.push(G_END);
        self.buf.extend(data);
        self.buf
    }
}

/// One attribute from a response: (group tag, which group, name, value).
type Attr = (u8, usize, String, Vec<u8>);

/// Status code and attributes of an IPP response.
fn parse(body: &[u8]) -> Result<(u16, Vec<Attr>), String> {
    let bad = || "the printing service gave an unreadable answer".to_string();
    if body.len() < 8 {
        return Err(bad());
    }
    let status = u16::from_be_bytes([body[2], body[3]]);
    let mut i = 8;
    let mut group = 0u8;
    let mut group_no = 0usize;
    let mut name = String::new();
    let mut out = Vec::new();
    while i < body.len() {
        let tag = body[i];
        i += 1;
        if tag < 0x10 {
            if tag == G_END {
                break;
            }
            group = tag;
            group_no += 1;
            continue;
        }
        let rd = |i: &mut usize| -> Result<Vec<u8>, String> {
            let n = u16::from_be_bytes([
                *body.get(*i).ok_or_else(bad)?,
                *body.get(*i + 1).ok_or_else(bad)?,
            ]) as usize;
            *i += 2;
            let v = body.get(*i..*i + n).ok_or_else(bad)?.to_vec();
            *i += n;
            Ok(v)
        };
        let n = rd(&mut i)?;
        if !n.is_empty() {
            name = String::from_utf8_lossy(&n).into_owned();
        }
        let v = rd(&mut i)?;
        let _ = tag;
        out.push((group, group_no, name.clone(), v));
    }
    Ok((status, out))
}

fn connect() -> Result<Box<dyn ReadWrite>, String> {
    for s in SOCKETS {
        if let Ok(c) = std::os::unix::net::UnixStream::connect(s) {
            c.set_read_timeout(Some(Duration::from_secs(60))).ok();
            return Ok(Box::new(c));
        }
    }
    let addr = "127.0.0.1:631".parse().unwrap();
    let c = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|_| "no printing service (CUPS) is running".to_string())?;
    c.set_read_timeout(Some(Duration::from_secs(60))).ok();
    Ok(Box::new(c))
}

trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

/// Sends one IPP request over HTTP and returns the IPP response body.
fn call(path: &str, ipp: &[u8]) -> Result<(u16, Vec<Attr>), String> {
    let mut c = connect()?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/ipp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ipp.len()
    );
    c.write_all(head.as_bytes())
        .and_then(|_| c.write_all(ipp))
        .map_err(|e| format!("could not reach the printing service: {e}"))?;
    let mut resp = Vec::new();
    c.read_to_end(&mut resp)
        .map_err(|e| format!("the printing service did not answer: {e}"))?;
    let split = resp
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("the printing service gave an unreadable answer")?;
    let headers = String::from_utf8_lossy(&resp[..split]).to_lowercase();
    if !headers.starts_with("http/1.1 200") && !headers.starts_with("http/1.0 200") {
        let line = headers.lines().next().unwrap_or("").to_string();
        return Err(format!("the printing service refused: {line}"));
    }
    let mut body = resp[split + 4..].to_vec();
    if headers.contains("transfer-encoding: chunked") {
        body = dechunk(&body);
    }
    parse(&body)
}

fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(eol) = b.windows(2).position(|w| w == b"\r\n") {
        let size = std::str::from_utf8(&b[..eol])
            .ok()
            .and_then(|s| usize::from_str_radix(s.split(';').next().unwrap_or("").trim(), 16).ok())
            .unwrap_or(0);
        if size == 0 {
            break;
        }
        let start = eol + 2;
        let Some(chunk) = b.get(start..start + size) else {
            break;
        };
        out.extend(chunk);
        b = b.get(start + size + 2..).unwrap_or(&[]);
    }
    out
}

fn text(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

/// The printers CUPS knows, and the default one's name.
pub fn printers() -> Result<(Vec<Printer>, Option<String>), String> {
    let mut r = Request::new(0x4002); // CUPS-Get-Printers
    r.attr(T_KEYWORD, "requested-attributes", b"printer-name")
        .more(T_KEYWORD, b"printer-info");
    let (status, attrs) = call("/", &r.finish(&[]))?;
    if status > 0xff && status != 0x0406 {
        return Err(format!(
            "the printing service answered with error {status:#06x}"
        ));
    }
    // One printer-attributes group per printer.
    let mut by_group: Vec<(usize, Printer)> = Vec::new();
    for (group, no, name, v) in &attrs {
        if *group != G_PRINTER {
            continue;
        }
        if by_group.last().is_none_or(|(n, _)| n != no) {
            by_group.push((
                *no,
                Printer {
                    name: String::new(),
                    info: String::new(),
                },
            ));
        }
        let p = &mut by_group.last_mut().unwrap().1;
        match name.as_str() {
            "printer-name" => p.name = text(v),
            "printer-info" => p.info = text(v),
            _ => {}
        }
    }
    let list: Vec<Printer> = by_group
        .into_iter()
        .map(|(_, p)| p)
        .filter(|p| !p.name.is_empty())
        .collect();
    let default = Request::new(0x4001) // CUPS-Get-Default
        .finish(&[]);
    let default = call("/", &default).ok().and_then(|(_, a)| {
        a.into_iter()
            .find(|(g, _, n, _)| *g == G_PRINTER && n == "printer-name")
            .map(|(.., v)| text(&v))
    });
    Ok((list, default))
}

/// Sends a PDF to a printer.
pub fn print(printer: &str, pdf: &[u8], title: &str, copies: u32) -> Result<(), String> {
    let uri = format!("ipp://localhost/printers/{printer}");
    let user = std::env::var("USER").unwrap_or_else(|_| "pdfr".into());
    let mut r = Request::new(0x0002); // Print-Job
    r.attr(T_URI, "printer-uri", uri.as_bytes())
        .attr(T_NAME, "requesting-user-name", user.as_bytes())
        .attr(T_NAME, "job-name", title.as_bytes())
        .attr(T_MIME, "document-format", b"application/pdf")
        .group(G_JOB)
        .attr(T_INTEGER, "copies", &copies.clamp(1, 999).to_be_bytes());
    let (status, _) = call(&format!("/printers/{printer}"), &r.finish(pdf))?;
    if status <= 0xff {
        Ok(())
    } else {
        Err(format!(
            "the printer did not accept the document (error {status:#06x})"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_requests() {
        let mut r = Request::new(0x0002);
        r.attr(T_URI, "printer-uri", b"ipp://x");
        let b = r.finish(b"%PDF");
        assert_eq!(&b[..4], &[1, 1, 0, 2]);
        assert_eq!(b[8], G_OPERATION);
        assert!(b.windows(11).any(|w| w == b"printer-uri"));
        assert!(b.ends_with(&[G_END, b'%', b'P', b'D', b'F']));
    }

    fn put(b: &mut Vec<u8>, tag: u8, name: &str, v: &str) {
        b.push(tag);
        b.extend((name.len() as u16).to_be_bytes());
        b.extend(name.as_bytes());
        b.extend((v.len() as u16).to_be_bytes());
        b.extend(v.as_bytes());
    }

    #[test]
    fn parses_printer_groups() {
        let mut b = vec![1, 1, 0, 0, 0, 0, 0, 1, G_OPERATION];
        put(&mut b, T_CHARSET, "attributes-charset", "utf-8");
        b.push(G_PRINTER);
        put(&mut b, T_NAME, "printer-name", "HP");
        b.push(G_PRINTER);
        put(&mut b, 0x41, "printer-info", "Office printer");
        put(&mut b, T_NAME, "printer-name", "Office");
        b.push(G_END);
        let (status, attrs) = parse(&b).unwrap();
        assert_eq!(status, 0);
        let groups: Vec<_> = attrs
            .iter()
            .filter(|a| a.0 == G_PRINTER)
            .map(|a| a.1)
            .collect();
        assert_eq!(groups, vec![2, 3, 3]);
        assert!(parse(&[1, 1]).is_err());
    }

    #[test]
    fn dechunks() {
        assert_eq!(dechunk(b"3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n"), b"abcde");
    }

    #[test]
    fn printer_labels() {
        let p = Printer {
            name: "HP_OfficeJet".into(),
            info: String::new(),
        };
        assert_eq!(p.label(), "HP_OfficeJet");
    }
}

#[cfg(test)]
mod live {
    /// Lists the local printers (prints nothing):
    /// `cargo test -p pdfr cups_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cups_live() {
        println!("{:?}", super::printers());
    }
}
