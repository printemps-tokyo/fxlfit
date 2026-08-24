//! Reading an EPUB container into the model.
//!
//! Nothing in here judges the book. When something cannot be read the reader
//! records a note and carries on with what it has, because a package document
//! that half parses still tells you which pages are missing a viewport.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use zip::ZipArchive;

use crate::image;
use crate::model::*;
use crate::page;
use crate::util;
use crate::xml::{self, Node};

/// How much of a resource is read to sniff its intrinsic size. A JPEG frame
/// header can sit behind a large EXIF or ICC segment, so the window is
/// generous; the rest of the file is never touched.
const SNIFF_BYTES: usize = 1 << 20;

/// What one read of an image resource yields: its intrinsic size, when the
/// header could be sniffed, and its size in the container.
type ImageFacts = (Option<(u32, u32)>, Option<u64>);

pub fn read(path: &Path) -> Result<Book> {
    let file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let file_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("{} is not a readable ZIP container", path.display()))?;

    let mut warnings: Vec<String> = Vec::new();
    let mut resources: BTreeMap<String, Resource> = BTreeMap::new();
    let mut total_bytes = 0u64;

    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|e| anyhow!("cannot read entry {i}: {e}"))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let bytes = entry.size();
        total_bytes += bytes;
        resources.insert(
            name.clone(),
            Resource {
                path: name,
                bytes,
                stored_bytes: entry.compressed_size(),
            },
        );
    }

    let encrypted = resources.contains_key("META-INF/encryption.xml");

    let container = read_entry(&mut archive, "META-INF/container.xml", usize::MAX)
        .ok_or_else(|| anyhow!("META-INF/container.xml is missing: this is not an EPUB"))?;
    let opf_path = rootfile_path(&container)
        .ok_or_else(|| anyhow!("META-INF/container.xml declares no rootfile"))?;

    let opf_bytes = read_entry(&mut archive, &opf_path, usize::MAX).ok_or_else(|| {
        anyhow!("package document {opf_path} is declared but not in the container")
    })?;
    let (opf_text, note) = xml::decode(&opf_bytes);
    if let Some(note) = note {
        warnings.push(format!("{opf_path}: {note}"));
    }
    let package = parse_package(&opf_path, &opf_text, &mut warnings);

    let global_layout = match package.meta_values("rendition:layout").first().copied() {
        Some("pre-paginated") => Layout::PrePaginated,
        _ => Layout::Reflowable,
    };

    // Spine order is reading order; index is 1-based so the report counts
    // pages the way a person does.
    let mut pages: Vec<Page> = Vec::new();
    let mut image_cache: BTreeMap<String, ImageFacts> = BTreeMap::new();

    for (n, spine_ref) in package.spine.refs.iter().enumerate() {
        let Some(item) = package.item_by_id(&spine_ref.idref) else {
            warnings.push(format!(
                "spine references idref \"{}\", which is not in the manifest",
                spine_ref.idref
            ));
            continue;
        };

        let layout = if spine_ref
            .properties
            .iter()
            .any(|p| p == "rendition:layout-pre-paginated")
        {
            Layout::PrePaginated
        } else if spine_ref
            .properties
            .iter()
            .any(|p| p == "rendition:layout-reflowable")
        {
            Layout::Reflowable
        } else {
            global_layout
        };

        let mut parsed = match read_entry(&mut archive, &item.path, usize::MAX) {
            Some(bytes) => {
                let (text, note) = xml::decode(&bytes);
                if let Some(note) = note {
                    warnings.push(format!("{}: {note}", item.path));
                }
                page::parse(&item.path, &item.media_type, &text)
            }
            None => page::PageParse {
                parse_error: Some("content document is not in the container".to_string()),
                ..page::PageParse::default()
            },
        };

        for img in parsed.images.iter_mut() {
            if img.remote {
                continue;
            }
            let entry = image_cache.entry(img.path.clone()).or_insert_with(|| {
                let bytes = resources.get(&img.path).map(|r| r.bytes);
                let head = read_entry(&mut archive, &img.path, SNIFF_BYTES);
                let px = head.as_deref().and_then(image::dimensions);
                (px, bytes)
            });
            img.pixels = entry.0;
            img.bytes = entry.1;
        }

        pages.push(Page {
            index: n + 1,
            idref: spine_ref.idref.clone(),
            path: item.path.clone(),
            media_type: item.media_type.clone(),
            linear: spine_ref.linear,
            spine_properties: spine_ref.properties.clone(),
            layout,
            viewport: parsed.viewport,
            viewport_source: parsed.viewport_source,
            viewport_raw: parsed.viewport_raw,
            title: parsed.title,
            lang: parsed.lang,
            images: parsed.images,
            text_len: parsed.text_len,
            parse_error: parsed.parse_error,
        });
    }

    // A book that forgets rendition:layout is still a comic, and hiding every
    // layout finding behind that one missing line would make the report
    // useless exactly when it is most needed. So when the pages themselves
    // are unmistakably fixed-layout -- a viewport and a single image, most of
    // the way through the spine -- the layout checks run against them, and
    // the missing declaration is reported on its own.
    let mut layout_inferred = false;
    if global_layout == Layout::Reflowable
        && pages.iter().all(|p| p.layout == Layout::Reflowable)
        && !pages.is_empty()
    {
        let looks_fixed = pages
            .iter()
            .filter(|p| p.viewport.is_some() && p.is_image_only())
            .count();
        if looks_fixed * 10 >= pages.len() * 6 {
            layout_inferred = true;
            for page in pages.iter_mut() {
                page.layout = Layout::PrePaginated;
            }
        }
    }

    let nav = package
        .items
        .iter()
        .find(|i| i.has_property("nav"))
        .and_then(|item| {
            read_entry(&mut archive, &item.path, usize::MAX).map(|bytes| {
                let (text, _) = xml::decode(&bytes);
                parse_nav(&item.path, &text)
            })
        });

    Ok(Book {
        file: path.display().to_string(),
        opf_path,
        package,
        pages,
        nav,
        resources,
        read_warnings: warnings,
        encrypted,
        total_bytes,
        file_bytes,
        layout_inferred,
    })
}

/// Read an entry by container path, up to `limit` bytes.
///
/// Entry names are matched exactly first, then case-insensitively, because a
/// container built on a case-insensitive filesystem can disagree with its own
/// package document about `Images/` versus `images/` -- which readers on Linux
/// then fail to open.
fn read_entry<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    path: &str,
    limit: usize,
) -> Option<Vec<u8>> {
    let name = match archive.index_for_name(path) {
        Some(idx) => idx,
        None => {
            let lower = path.to_ascii_lowercase();
            let mut found = None;
            for i in 0..archive.len() {
                let entry = archive.by_index(i).ok()?;
                if entry.name().to_ascii_lowercase() == lower {
                    found = Some(i);
                    break;
                }
            }
            found?
        }
    };

    let mut entry = archive.by_index(name).ok()?;
    let cap = entry.size().min(limit as u64) as usize;
    let mut buf = Vec::with_capacity(cap.min(SNIFF_BYTES));
    let mut reader = (&mut entry).take(limit as u64);
    reader.read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn rootfile_path(container: &[u8]) -> Option<String> {
    let (text, _) = xml::decode(container);
    let mut full_path = None;
    xml::scan(&text, |node| {
        if let Node::Start(e) = node {
            if e.name == "rootfile" && full_path.is_none() {
                if let Some(p) = e.attr("full-path") {
                    full_path = Some(util::percent_decode(p.trim()));
                }
            }
        }
    });
    full_path
}

fn parse_package(opf_path: &str, text: &str, warnings: &mut Vec<String>) -> Package {
    let mut pkg = Package::default();
    let mut stack: Vec<String> = Vec::new();
    let mut current_dc: Option<DcEntry> = None;
    let mut current_meta: Option<MetaEntry> = None;

    let note = xml::scan(text, |node| match node {
        Node::Start(e) => {
            match e.name.as_str() {
                "package" => {
                    pkg.version = e.attr("version").map(|s| s.trim().to_string());
                    pkg.unique_identifier = e.attr("unique-identifier").map(|s| s.to_string());
                    pkg.prefix = e.attr("prefix").map(|s| s.to_string());
                }
                "item" if stack.iter().any(|s| s == "manifest") => {
                    let href_raw = e.attr("href").unwrap_or("").trim().to_string();
                    let remote = util::is_remote(&href_raw);
                    let path = if remote {
                        href_raw.clone()
                    } else {
                        util::resolve(opf_path, &href_raw)
                    };
                    pkg.items.push(Item {
                        id: e.attr("id").unwrap_or("").to_string(),
                        href_raw,
                        path,
                        media_type: e.attr("media-type").unwrap_or("").trim().to_string(),
                        properties: e.attr_tokens("properties"),
                        fallback: e.attr("fallback").map(|s| s.to_string()),
                        remote,
                    });
                }
                "spine" => {
                    pkg.spine.page_progression_direction = e
                        .attr("page-progression-direction")
                        .map(|s| s.trim().to_ascii_lowercase());
                    pkg.spine.toc = e.attr("toc").map(|s| s.to_string());
                }
                "itemref" => {
                    pkg.spine.refs.push(SpineRef {
                        idref: e.attr("idref").unwrap_or("").to_string(),
                        linear: e.attr("linear").map(|v| v != "no").unwrap_or(true),
                        properties: e.attr_tokens("properties"),
                    });
                }
                "meta" if stack.iter().any(|s| s == "metadata") => {
                    let entry = MetaEntry {
                        property: e.attr("property").map(|s| s.trim().to_string()),
                        name: e.attr("name").map(|s| s.trim().to_string()),
                        content: e.attr("content").map(|s| s.to_string()),
                        refines: e.attr("refines").map(|s| s.trim().to_string()),
                        id: e.attr("id").map(|s| s.to_string()),
                        scheme: e.attr("scheme").map(|s| s.to_string()),
                        text: String::new(),
                    };
                    if e.empty {
                        pkg.meta.push(entry);
                    } else {
                        current_meta = Some(entry);
                    }
                }
                _ => {
                    // Dublin Core elements live in the dc namespace; match on
                    // the prefix so a package that binds it differently is
                    // still read.
                    if stack.iter().any(|s| s == "metadata")
                        && e.prefix.as_deref() == Some("dc")
                        && current_dc.is_none()
                    {
                        current_dc = Some(DcEntry {
                            name: e.name.clone(),
                            value: String::new(),
                            id: e.attr("id").map(|s| s.to_string()),
                        });
                    }
                }
            }
            if !e.empty {
                stack.push(e.name.clone());
            }
        }
        Node::Text(t) => {
            if let Some(dc) = current_dc.as_mut() {
                dc.value.push_str(&t);
            } else if let Some(meta) = current_meta.as_mut() {
                meta.text.push_str(&t);
            }
        }
        Node::End(name) => {
            if let Some(dc) = current_dc.take() {
                if dc.name == name {
                    pkg.dc.push(DcEntry {
                        value: xml::collapse(&dc.value),
                        ..dc
                    });
                } else {
                    current_dc = Some(dc);
                }
            }
            if name == "meta" {
                if let Some(meta) = current_meta.take() {
                    pkg.meta.push(MetaEntry {
                        text: xml::collapse(&meta.text),
                        ..meta
                    });
                }
            }
            if let Some(pos) = stack.iter().rposition(|s| *s == name) {
                stack.truncate(pos);
            }
        }
    });

    if let Some(note) = note {
        warnings.push(format!("{opf_path}: {note}"));
    }
    pkg
}

fn parse_nav(path: &str, text: &str) -> Nav {
    let mut nav = Nav {
        path: path.to_string(),
        ..Nav::default()
    };
    let mut current_types: Vec<String> = Vec::new();
    let mut nav_depth = 0usize;

    xml::scan(text, |node| match node {
        Node::Start(e) => {
            if e.name == "nav" {
                nav_depth += 1;
                let types = e
                    .attr("epub:type")
                    .or_else(|| e.attr("type"))
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if types.split_whitespace().any(|t| t == "page-list") {
                    nav.has_page_list = true;
                }
                if types.split_whitespace().any(|t| t == "landmarks") {
                    nav.has_landmarks = true;
                }
                current_types = types.split_whitespace().map(|s| s.to_string()).collect();
            }
            if e.name == "a" && current_types.iter().any(|t| t == "toc") && nav_depth > 0 {
                nav.toc_entries += 1;
            }
        }
        Node::End(name) => {
            if name == "nav" {
                nav_depth = nav_depth.saturating_sub(1);
                if nav_depth == 0 {
                    current_types.clear();
                }
            }
        }
        Node::Text(_) => {}
    });

    nav
}

/// Every container path a content document points at, used to find resources
/// nothing references.
pub fn referenced_paths(book: &Book) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for page in &book.pages {
        set.insert(page.path.clone());
        for img in &page.images {
            if !img.remote {
                set.insert(img.path.clone());
            }
        }
    }
    set
}
