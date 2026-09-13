//! A very small pull-scanner over quick-xml.
//!
//! The checks only ever ask three things of a document: which elements are
//! there, what attributes do they carry, and what text does it render. So the
//! reader stays lenient on purpose -- unmatched end tags, unknown entities and
//! stray ampersands are reported as a note rather than aborting the scan,
//! because a malformed content document is exactly the kind of book this tool
//! is pointed at.

use std::collections::BTreeMap;

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use quick_xml::XmlVersion;

/// An element as the scanner hands it over.
#[derive(Debug, Clone)]
pub struct Element {
    /// Local name, lowercased, without the namespace prefix (`opf:item` ->
    /// `item`).
    pub name: String,
    /// Namespace prefix as authored, lowercased (`opf` for `opf:item`).
    pub prefix: Option<String>,
    /// Attributes keyed by the name as authored, lowercased.
    pub attrs: BTreeMap<String, String>,
    /// True when the element was written in the self-closing form.
    pub empty: bool,
}

impl Element {
    /// Attribute lookup that accepts either the authored name or the local
    /// part, so `xml:lang` is found by `lang` and `epub:type` by `type`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        if let Some(v) = self.attrs.get(name) {
            return Some(v.as_str());
        }
        self.attrs
            .iter()
            .find(|(k, _)| local_part(k) == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whitespace-separated attribute value, as EPUB uses for `properties`.
    pub fn attr_tokens(&self, name: &str) -> Vec<String> {
        self.attr(name)
            .map(|v| v.split_whitespace().map(|t| t.to_string()).collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum Node {
    Start(Element),
    End(String),
    Text(String),
}

fn local_part(name: &str) -> &str {
    match name.split_once(':') {
        Some((_, local)) => local,
        None => name,
    }
}

/// Decode container bytes into text.
///
/// EPUB requires XML documents to be UTF-8 or UTF-16; anything else is
/// decoded as UTF-8 with replacement so the scan can still say something
/// useful about the file, and the caller is told that happened.
pub fn decode(bytes: &[u8]) -> (String, Option<String>) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return decode_utf8(&bytes[3..]);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (decode_utf16(&bytes[2..], true), None);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return (decode_utf16(&bytes[2..], false), None);
    }
    decode_utf8(bytes)
}

fn decode_utf8(bytes: &[u8]) -> (String, Option<String>) {
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), None),
        Err(_) => (
            String::from_utf8_lossy(bytes).into_owned(),
            Some("not valid UTF-8; decoded with replacement characters".to_string()),
        ),
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    // Pairs are taken by index rather than through `chunks_exact`, which
    // newer clippy asks to be written as `as_chunks` -- an API too recent to
    // rely on here. A trailing odd byte is dropped, as it cannot start a
    // code unit.
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let pair = [bytes[i], bytes[i + 1]];
        units.push(if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        });
        i += 2;
    }
    String::from_utf16_lossy(&units)
}

/// Walk a document, handing every element, end tag and text run to `sink`.
///
/// Returns a note when the parser gave up part way through; the nodes seen
/// before that point have already been delivered.
pub fn scan(text: &str, mut sink: impl FnMut(Node)) -> Option<String> {
    let mut reader = Reader::from_str(text);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.check_comments = false;
    config.allow_unmatched_ends = true;
    config.allow_dangling_amp = true;

    loop {
        match reader.read_event() {
            Ok(Event::Eof) => return None,
            Ok(Event::Start(e)) => sink(Node::Start(element(&e, false))),
            Ok(Event::Empty(e)) => sink(Node::Start(element(&e, true))),
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_lowercase();
                sink(Node::End(local_part(&name).to_string()));
            }
            Ok(Event::Text(e)) => {
                // Entity references are resolved where they are known; an
                // unknown one (an XHTML named entity, say) is left as authored
                // rather than dropping the run of text that carries it.
                sink(Node::Text(e.xml10_content().into_owned()));
            }
            Ok(Event::CData(e)) => sink(Node::Text(String::from(&*e))),
            Ok(_) => {}
            Err(err) => {
                return Some(format!(
                    "XML parse stopped at byte {}: {err}",
                    reader.buffer_position()
                ))
            }
        }
    }
}

fn element(e: &quick_xml::events::BytesStart<'_>, empty: bool) -> Element {
    let qname = e.name().as_ref().to_lowercase();
    let (prefix, name) = match qname.split_once(':') {
        Some((p, l)) => (Some(p.to_string()), l.to_string()),
        None => (None, qname.clone()),
    };

    let mut attrs = BTreeMap::new();
    for attr in e.attributes() {
        let Ok(attr) = attr else { continue };
        let key = attr.key.as_ref().to_lowercase();
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| attr.value.as_ref().to_string());
        attrs.insert(key, value);
    }

    Element {
        name,
        prefix,
        attrs,
        empty,
    }
}

/// Collapse a run of text the way a renderer would, so "is this page empty"
/// is not decided by indentation.
pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
