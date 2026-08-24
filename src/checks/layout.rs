//! Fixed-layout declarations.
//!
//! A comic or a picture book is only fixed-layout because the package says
//! so. Get that wrong and a reading system reflows the artwork, or scales it
//! against a viewport that does not match the canvas the pages were drawn on.
//! These checks read what the book declares and compare it with what the
//! pages actually are.

use std::collections::BTreeMap;

use super::catalog::*;
use super::{page_label, Options};
use crate::model::{Book, Finding, Layout, Severity, ViewportSource};
use crate::util;

pub fn check(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    layout_declaration(book, out);
    viewports(book, out);
    spread(book, out);
    progression(book, out);
    deprecated_and_conflicting(book, out);
    image_fit(book, opts, out);
}

fn layout_declaration(book: &Book, out: &mut Vec<Finding>) {
    let fxl = book.fxl_pages().count();
    let total = book.pages.len();

    if book.layout_inferred {
        out.push(
            Finding::new(
                &FXL001,
                "rendition:layout pre-paginated is not declared",
                format!(
                    "the package declares no layout, so all {total} spine page(s) reflow -- but \
                     {fxl} of them are a single image with a viewport, which is a fixed-layout \
                     page in everything but the declaration, so the layout checks below were \
                     run against them anyway"
                ),
                "add <meta property=\"rendition:layout\">pre-paginated</meta> to the package \
                 metadata; without it a reading system stretches the artwork to fit whatever \
                 window it has",
            )
            .at([book.opf_path.clone()]),
        );
    } else if fxl == 0 {
        out.push(
            Finding::new(
                &FXL001,
                "no page in the spine is pre-paginated",
                format!(
                    "rendition:layout is {} and no itemref overrides it, so all {total} spine \
                     item(s) reflow",
                    book.declared_layout()
                        .map(|l| l.label().to_string())
                        .unwrap_or_else(|| "not declared (defaults to reflowable)".to_string())
                ),
                "add <meta property=\"rendition:layout\">pre-paginated</meta> to the package \
                 metadata -- or, if the book really is reflowable, this is the wrong tool for it",
            )
            .at([book.opf_path.clone()]),
        );
        return;
    }

    if book.declared_layout().is_none() && !book.layout_inferred {
        out.push(
            Finding::new(
                &FXL001,
                "rendition:layout is not declared at package level",
                format!(
                    "{fxl} of {total} spine item(s) are pre-paginated only because their \
                     itemref says so; a reading system that ignores itemref properties reflows \
                     the rest"
                ),
                "declare <meta property=\"rendition:layout\">pre-paginated</meta> once in the \
                 package metadata and override per page only where you mean to",
            )
            .at([book.opf_path.clone()])
            .with_severity(Severity::Warn),
        );
    }

    if fxl != total {
        let reflowable: Vec<String> = book
            .pages
            .iter()
            .filter(|p| p.layout == Layout::Reflowable)
            .map(page_label)
            .collect();
        out.push(
            Finding::new(
                &FXL002,
                format!(
                    "the spine mixes layouts: {fxl} pre-paginated, {} reflowable",
                    reflowable.len()
                ),
                util::summarize_list(&reflowable, 6),
                "mixing is legal and sometimes deliberate (a reflowable colophon after fixed \
                 pages); confirm each reflowable page is one you meant",
            )
            .at(reflowable),
        );
    }
}

fn viewports(book: &Book, out: &mut Vec<Finding>) {
    let mut missing: Vec<String> = Vec::new();
    let mut malformed: Vec<String> = Vec::new();
    let mut sizes: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();

    for page in book.fxl_pages() {
        match (page.viewport, page.viewport_source) {
            (Some(vp), _) => sizes.entry(vp).or_default().push(page_label(page)),
            (None, ViewportSource::None) => missing.push(page_label(page)),
            (None, _) => malformed.push(format!(
                "{} declares \"{}\"",
                page_label(page),
                page.viewport_raw.clone().unwrap_or_default()
            )),
        }
    }

    if !missing.is_empty() {
        out.push(
            Finding::new(
                &FXL003,
                format!("{} fixed-layout page(s) declare no viewport", missing.len()),
                util::summarize_list(&missing, 8),
                "add <meta name=\"viewport\" content=\"width=W, height=H\"/> to every XHTML \
                 page, in the pixel size the artwork was drawn at; an SVG page needs width and \
                 height, or a viewBox",
            )
            .at(missing),
        );
    }

    if !malformed.is_empty() {
        out.push(
            Finding::new(
                &FXL011,
                format!(
                    "{} viewport declaration(s) carry no pixel size",
                    malformed.len()
                ),
                util::summarize_list(&malformed, 8),
                "a fixed-layout page needs concrete pixels: width=device-width, percentages and \
                 initial-scale alone leave the reading system nothing to scale the page against",
            )
            .at(malformed),
        );
    }

    if sizes.len() > 1 {
        // A spread page authored at double width is a normal thing to find in
        // a comic, so it is named rather than counted against the book.
        let dominant = sizes
            .iter()
            .max_by_key(|(_, pages)| pages.len())
            .map(|(size, _)| *size)
            .unwrap_or((0, 0));
        let mut odd: Vec<String> = Vec::new();
        let mut spreads = 0usize;
        for (size, pages) in &sizes {
            if *size == dominant {
                continue;
            }
            if size.1 == dominant.1 && size.0 == dominant.0 * 2 {
                spreads += pages.len();
                continue;
            }
            odd.push(format!(
                "{}x{} on {}",
                size.0,
                size.1,
                util::summarize_list(pages, 3)
            ));
        }
        if !odd.is_empty() {
            let mut detail = format!(
                "most pages are {}x{}; also found {}",
                dominant.0,
                dominant.1,
                util::summarize_list(&odd, 6)
            );
            if spreads > 0 {
                detail.push_str(&format!(
                    " ({spreads} double-width page(s) treated as spreads and not counted)"
                ));
            }
            out.push(
                Finding::new(
                    &FXL004,
                    format!("{} distinct viewport size(s) across the book", sizes.len()),
                    detail,
                    "author every page at one canvas size, or accept that readers see the page \
                     size jump mid-book; double-width spreads are the one normal exception",
                )
                .at(odd),
            );
        }
    }
}

fn spread(book: &Book, out: &mut Vec<Finding>) {
    if book.fxl_pages().count() == 0 {
        return;
    }

    let declared = book.package.meta_values("rendition:spread");
    let any_itemref_spread = book.pages.iter().any(|p| {
        p.spine_properties
            .iter()
            .any(|x| x.starts_with("rendition:spread-"))
    });

    if declared.is_empty() && !any_itemref_spread {
        out.push(
            Finding::new(
                &FXL006,
                "rendition:spread is not declared",
                "the reading system falls back to auto, so whether two pages are shown side by \
                 side is decided by the device, not by the book",
                "declare <meta property=\"rendition:spread\">both</meta> for a book meant to be \
                 read in spreads, or none for one that is not",
            )
            .at([book.opf_path.clone()]),
        );
    }

    // page-spread-left / page-spread-right decide which half of a synthetic
    // spread a page lands on. Declaring them on some pages and not others, or
    // repeating the same side twice in a row, shifts every later spread.
    let linear: Vec<&crate::model::Page> = book
        .pages
        .iter()
        .filter(|p| p.linear && p.layout == Layout::PrePaginated)
        .collect();

    let side = |p: &crate::model::Page| -> Option<&'static str> {
        if p.has_spine_property("page-spread-left") {
            Some("left")
        } else if p.has_spine_property("page-spread-right") {
            Some("right")
        } else if p.has_spine_property("rendition:page-spread-center") {
            Some("center")
        } else {
            None
        }
    };

    let with_side = linear.iter().filter(|p| side(p).is_some()).count();
    if with_side == 0 {
        return;
    }

    if with_side < linear.len() {
        let bare: Vec<String> = linear
            .iter()
            .filter(|p| side(p).is_none())
            .map(|p| page_label(p))
            .collect();
        out.push(
            Finding::new(
                &FXL007,
                format!(
                    "{} of {} pages declare a spread side, {} do not",
                    with_side,
                    linear.len(),
                    bare.len()
                ),
                util::summarize_list(&bare, 8),
                "declare page-spread-left / page-spread-right on every page, or on none and let \
                 the reading system alternate from the page progression direction",
            )
            .at(bare),
        );
        return;
    }

    let mut repeats: Vec<String> = Vec::new();
    for pair in linear.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        match (side(a), side(b)) {
            (Some(x), Some(y)) if x == y && x != "center" => {
                repeats.push(format!(
                    "{} and {} are both page-spread-{x}",
                    page_label(a),
                    page_label(b)
                ));
            }
            _ => {}
        }
    }
    if !repeats.is_empty() {
        out.push(
            Finding::new(
                &FXL007,
                format!("{} spread side(s) do not alternate", repeats.len()),
                util::summarize_list(&repeats, 6),
                "two consecutive pages on the same side push every following spread one page \
                 out; check for a missing blank page around the run",
            )
            .at(repeats),
        );
    }
}

fn progression(book: &Book, out: &mut Vec<Finding>) {
    if book.fxl_pages().count() == 0 {
        return;
    }
    let languages: Vec<String> = book
        .package
        .dc_values("language")
        .iter()
        .map(|l| l.to_ascii_lowercase())
        .collect();
    let rtl_language = languages.iter().any(|l| {
        l.starts_with("ja")
            || l.starts_with("ar")
            || l.starts_with("he")
            || l.starts_with("fa")
            || l.starts_with("ur")
    });

    match book.package.spine.page_progression_direction.as_deref() {
        None => {
            let mut detail =
                "the spine carries no page-progression-direction, so the reading system assumes \
                 left to right"
                    .to_string();
            if rtl_language {
                detail.push_str(&format!(
                    "; dc:language is {} -- right-to-left is the usual reading order for it, and \
                     for manga it also decides which page of a spread is which",
                    languages.join(", ")
                ));
            }
            out.push(
                Finding::new(
                    &FXL008,
                    "page-progression-direction is not declared",
                    detail,
                    "add page-progression-direction=\"rtl\" (or \"ltr\") to the spine element and \
                     say it explicitly",
                )
                .at([book.opf_path.clone()])
                .with_severity(if rtl_language {
                    Severity::Error
                } else {
                    Severity::Warn
                }),
            );
        }
        Some(dir) if !matches!(dir, "ltr" | "rtl" | "default") => {
            out.push(
                Finding::new(
                    &FXL008,
                    format!("page-progression-direction=\"{dir}\" is not a valid value"),
                    "EPUB 3.3 allows ltr, rtl and default".to_string(),
                    "use ltr, rtl or default",
                )
                .at([book.opf_path.clone()])
                .with_severity(Severity::Error),
            );
        }
        Some(dir) if dir == "ltr" && rtl_language => {
            out.push(
                Finding::new(
                    &FXL008,
                    "page progression is ltr on a right-to-left language book",
                    format!("dc:language is {}", languages.join(", ")),
                    "confirm this is deliberate; a manga read left to right shows every spread \
                     in the wrong order",
                )
                .at([book.opf_path.clone()]),
            );
        }
        _ => {}
    }
}

fn deprecated_and_conflicting(book: &Book, out: &mut Vec<Finding>) {
    if book.fxl_pages().count() == 0 {
        return;
    }

    if !book.package.meta_values("rendition:viewport").is_empty() {
        out.push(
            Finding::new(
                &FXL010,
                "the deprecated rendition:viewport property is used",
                "EPUB 3.3 deprecates rendition:viewport; the viewport of a fixed-layout page is \
                 the one declared inside the page itself",
                "delete the package-level rendition:viewport and rely on the per-page viewport \
                 meta (or the SVG dimensions)",
            )
            .at([book.opf_path.clone()]),
        );
    }

    let flow = book.package.meta_values("rendition:flow");
    if !flow.is_empty()
        && (book.declared_layout() == Some(Layout::PrePaginated) || book.layout_inferred)
    {
        out.push(
            Finding::new(
                &FXL009,
                format!(
                    "rendition:flow=\"{}\" on a pre-paginated publication",
                    flow.join(", ")
                ),
                "flow describes how reflowable content is paginated; on a pre-paginated \
                 publication it has nothing to act on, and vertical-scroll delivery cannot be \
                 expressed this way"
                    .to_string(),
                "remove rendition:flow; for scrolling webtoon delivery, slice the strip into \
                 pages and let each page be one image",
            )
            .at([book.opf_path.clone()]),
        );
    }
}

fn image_fit(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    let mut mismatched: Vec<String> = Vec::new();

    for page in book.fxl_pages() {
        let (Some((vw, vh)), true) = (page.viewport, page.is_image_only()) else {
            continue;
        };
        let Some((iw, ih)) = page.images[0].pixels else {
            continue;
        };
        if vw == 0 || vh == 0 || iw == 0 || ih == 0 {
            continue;
        }
        let viewport_ratio = vw as f64 / vh as f64;
        let image_ratio = iw as f64 / ih as f64;
        let drift = (image_ratio - viewport_ratio).abs() / viewport_ratio * 100.0;
        if drift > opts.aspect_tolerance {
            mismatched.push(format!(
                "{}: viewport {vw}x{vh}, image {iw}x{ih} ({drift:.1}% off)",
                page_label(page)
            ));
        }
    }

    if !mismatched.is_empty() {
        out.push(
            Finding::new(
                &FXL005,
                format!(
                    "{} page image(s) do not fit their viewport",
                    mismatched.len()
                ),
                util::summarize_list(&mismatched, 8),
                format!(
                    "match the viewport to the artwork, pixel for pixel; anything off by more \
                     than {:.1}% is letterboxed or cropped by the reading system, differently on \
                     each one",
                    opts.aspect_tolerance
                ),
            )
            .at(mismatched),
        );
    }
}
