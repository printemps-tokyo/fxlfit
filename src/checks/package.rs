//! Package, container and navigation rules.
//!
//! These are the things that make a file an EPUB at all, checked here only as
//! far as a preflight needs them. This is not an EPUB validator: epubcheck
//! remains the tool that says whether the file is conformant, and the README
//! says so.

use std::collections::BTreeSet;

use super::catalog::*;
use super::Options;
use crate::epub;
use crate::model::{Book, Finding, Severity};
use crate::util;

pub fn check(book: &Book, _opts: &Options, out: &mut Vec<Finding>) {
    identity(book, out);
    navigation(book, out);
    manifest(book, out);
    spine(book, out);
    parse_errors(book, out);
}

fn identity(book: &Book, out: &mut Vec<Finding>) {
    let pkg = &book.package;
    let mut missing = Vec::new();
    for name in ["title", "language", "identifier"] {
        if pkg.dc_values(name).is_empty() {
            missing.push(format!("dc:{name}"));
        }
    }
    if !missing.is_empty() {
        out.push(
            Finding::new(
                &PKG001,
                "package metadata is incomplete",
                format!("missing: {}", missing.join(", ")),
                "add the missing Dublin Core elements to the package metadata; every store \
                 keys its catalogue off them",
            )
            .at([book.opf_path.clone()]),
        );
    }

    if pkg.meta_values("dcterms:modified").is_empty() {
        out.push(
            Finding::new(
                &PKG002,
                "dcterms:modified is missing",
                "no <meta property=\"dcterms:modified\"> in the package metadata",
                "add <meta property=\"dcterms:modified\">2026-08-24T00:00:00Z</meta> with the \
                 real last-modified timestamp in UTC",
            )
            .at([book.opf_path.clone()]),
        );
    }

    match pkg.version.as_deref() {
        Some(v) if v.starts_with('3') => {}
        Some(v) => out.push(
            Finding::new(
                &PKG011,
                format!("package declares EPUB version {v}"),
                "fixed-layout properties, the navigation document and accessibility metadata \
                 are EPUB 3 features; every check below assumes EPUB 3"
                    .to_string(),
                "publish as EPUB 3 (<package version=\"3.0\">) before relying on this report",
            )
            .at([book.opf_path.clone()]),
        ),
        None => out.push(
            Finding::new(
                &PKG011,
                "package declares no version",
                "the package element carries no version attribute",
                "add version=\"3.0\" to the package element",
            )
            .at([book.opf_path.clone()]),
        ),
    }

    if book
        .package
        .items
        .iter()
        .all(|i| !i.has_property("cover-image"))
    {
        out.push(
            Finding::new(
                &PKG004,
                "no cover image is declared",
                "no manifest item carries properties=\"cover-image\"",
                "mark the cover with properties=\"cover-image\" in the manifest; storefronts \
                 read that, not the first page of the spine",
            )
            .at([book.opf_path.clone()]),
        );
    }

    if book.encrypted {
        out.push(
            Finding::new(
                &PKG008,
                "the container carries META-INF/encryption.xml",
                "resources are encrypted or fonts are obfuscated; anything encrypted is opaque \
                 to this preflight, and to a reading system that does not implement the scheme",
                "confirm the encryption is font obfuscation you meant to ship, and that the \
                 store you are uploading to accepts it",
            )
            .at(["META-INF/encryption.xml".to_string()]),
        );
    }
}

fn navigation(book: &Book, out: &mut Vec<Finding>) {
    if book.nav.is_none() {
        out.push(
            Finding::new(
                &PKG003,
                "no navigation document",
                "no manifest item carries properties=\"nav\"",
                "add an EPUB 3 navigation document with a toc nav, and declare it with \
                 properties=\"nav\"",
            )
            .at([book.opf_path.clone()]),
        );
    }
}

fn manifest(book: &Book, out: &mut Vec<Finding>) {
    let mut missing: Vec<String> = Vec::new();
    let mut remote: Vec<String> = Vec::new();

    for item in &book.package.items {
        if item.remote {
            remote.push(format!("{} ({})", item.href_raw, item.id));
            continue;
        }
        if !book.resources.contains_key(&item.path) {
            missing.push(format!("{} (id={})", item.path, item.id));
        }
    }

    for page in &book.pages {
        for img in page.images.iter().filter(|i| i.remote) {
            remote.push(format!("{} in {}", img.path, page.path));
        }
    }

    if !missing.is_empty() {
        out.push(
            Finding::new(
                &PKG005,
                format!(
                    "{} manifest item(s) are not in the container",
                    missing.len()
                ),
                util::summarize_list(&missing, 8),
                "remove the stale manifest entries or add the files; a reading system that \
                 follows the manifest will fail on the first one it opens",
            )
            .at(missing),
        );
    }

    if !remote.is_empty() {
        out.push(
            Finding::new(
                &PKG007,
                format!("{} remote resource reference(s)", remote.len()),
                util::summarize_list(&remote, 8),
                "package the resource inside the container; a book read offline, or on a store \
                 that forbids network access, loses it",
            )
            .at(remote),
        );
    }

    // Anything in the container that no content document points at and the
    // manifest does not classify as navigation, cover or style is dead weight
    // in a format where every megabyte is shipped to every reader.
    let referenced = epub::referenced_paths(book);
    let mut orphans: Vec<String> = Vec::new();
    let mut orphan_bytes = 0u64;
    let structural: BTreeSet<&str> = ["mimetype", "META-INF/container.xml"].into_iter().collect();

    for (path, res) in &book.resources {
        if structural.contains(path.as_str()) || path.starts_with("META-INF/") {
            continue;
        }
        if path == &book.opf_path || referenced.contains(path) {
            continue;
        }
        let item = book.package.item_by_path(path);
        let keep = matches!(item, Some(i) if i.has_property("nav")
            || i.has_property("cover-image")
            || i.media_type == "text/css"
            || i.media_type.starts_with("font/")
            || i.media_type.starts_with("application/font")
            || i.media_type == "application/x-dtbncx+xml");
        if keep {
            continue;
        }
        orphans.push(format!("{path} ({})", util::human_bytes(res.bytes)));
        orphan_bytes += res.bytes;
    }

    if !orphans.is_empty() {
        out.push(
            Finding::new(
                &PKG006,
                format!(
                    "{} unreferenced resource(s), {} in total",
                    orphans.len(),
                    util::human_bytes(orphan_bytes)
                ),
                util::summarize_list(&orphans, 8),
                "delete them from the container, or reference them; note that images used only \
                 from CSS look unreferenced to this tool and to an accessibility tree alike",
            )
            .at(orphans),
        );
    }
}

fn spine(book: &Book, out: &mut Vec<Finding>) {
    if book.package.spine.refs.is_empty() {
        out.push(
            Finding::new(
                &PKG009,
                "the spine is empty",
                "no itemref elements: the book has no reading order",
                "list every page in the spine, in reading order",
            )
            .at([book.opf_path.clone()]),
        );
        return;
    }

    let dangling: Vec<String> = book
        .package
        .spine
        .refs
        .iter()
        .filter(|r| book.package.item_by_id(&r.idref).is_none())
        .map(|r| r.idref.clone())
        .collect();

    if !dangling.is_empty() {
        out.push(
            Finding::new(
                &PKG009,
                format!("{} spine reference(s) point at nothing", dangling.len()),
                format!(
                    "idrefs not in the manifest: {}",
                    util::summarize_list(&dangling, 8)
                ),
                "fix the idrefs, or drop the itemrefs",
            )
            .at([book.opf_path.clone()]),
        );
    }
}

fn parse_errors(book: &Book, out: &mut Vec<Finding>) {
    let broken: Vec<(&str, String)> = book
        .pages
        .iter()
        .filter_map(|p| p.parse_error.as_ref().map(|e| (p.path.as_str(), e.clone())))
        .collect();

    if broken.is_empty() {
        return;
    }
    let detail = broken
        .iter()
        .take(5)
        .map(|(p, e)| format!("{p}: {e}"))
        .collect::<Vec<_>>()
        .join("; ");
    out.push(
        Finding::new(
            &PKG010,
            format!("{} content document(s) could not be parsed", broken.len()),
            detail,
            "run epubcheck and fix the markup; what this report says about those pages is \
             whatever could be read before the parser stopped",
        )
        .at(broken
            .iter()
            .map(|(p, _)| p.to_string())
            .collect::<Vec<_>>())
        .with_severity(Severity::Error),
    );
}
