//! EPUB Accessibility 1.1 content and discovery metadata.
//!
//! Two different things are checked here and they fail in different ways.
//! The content side is about the pages: a comic page is one image, and if it
//! carries no text alternative there is nothing at all for a screen reader to
//! announce. The metadata side is about discovery: since the European
//! Accessibility Act took effect on 2025-06-28, a storefront selling into the
//! EU has to be able to state a book's accessibility characteristics, and it
//! reads them out of the package document.
//!
//! Where a rule is a judgement rather than a requirement -- alt text that is
//! technically present but says "page 12" -- it is reported as a warning and
//! the wording says what was seen, not what the author meant.

use std::collections::BTreeMap;

use super::catalog::*;
use super::{page_label, Options};
use crate::model::{Book, Finding, Page, Severity};
use crate::util;

pub fn check(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    alternatives(book, out);
    discovery_metadata(book, opts, out);
    conformance(book, opts, out);
    contradictions(book, out);
    documents(book, out);
    navigation(book, out);
}

fn is_decorative(img: &crate::model::PageImage) -> bool {
    img.aria_hidden
        || matches!(img.role.as_deref(), Some("presentation") | Some("none"))
        || img.alt.as_deref() == Some("")
}

fn alternatives(book: &Book, out: &mut Vec<Finding>) {
    let mut no_alt: Vec<String> = Vec::new();
    let mut hidden_pages: Vec<String> = Vec::new();
    let mut placeholders: Vec<String> = Vec::new();
    let mut by_alt: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for page in &book.pages {
        for img in &page.images {
            if page.is_image_only() && is_decorative(img) {
                hidden_pages.push(format!(
                    "{}: {} is {}",
                    page_label(page),
                    util::basename(&img.path),
                    if img.aria_hidden {
                        "aria-hidden=\"true\"".to_string()
                    } else if img.alt.as_deref() == Some("") {
                        "alt=\"\"".to_string()
                    } else {
                        format!("role=\"{}\"", img.role.clone().unwrap_or_default())
                    }
                ));
                continue;
            }
            match img.alt.as_deref() {
                None => {
                    if !is_decorative(img) {
                        no_alt.push(format!(
                            "{}: {}",
                            page_label(page),
                            util::basename(&img.path)
                        ));
                    }
                }
                Some(alt) if !alt.is_empty() => {
                    if let Some(reason) = placeholder_reason(alt, &img.path) {
                        placeholders.push(format!(
                            "{}: \"{}\" -- {reason}",
                            page_label(page),
                            truncate(alt, 60)
                        ));
                    }
                    by_alt
                        .entry(alt.trim().to_ascii_lowercase())
                        .or_default()
                        .push(page_label(page));
                }
                Some(_) => {}
            }
        }
    }

    if !no_alt.is_empty() {
        out.push(
            Finding::new(
                &A11Y001,
                format!("{} image(s) carry no text alternative", no_alt.len()),
                util::summarize_list(&no_alt, 8),
                "give every content image an alt attribute (or, for an SVG page, a <title> the \
                 image is described by); on a comic page the image is the page, so a missing alt \
                 leaves the reader with nothing",
            )
            .at(no_alt),
        );
    }

    if !hidden_pages.is_empty() {
        out.push(
            Finding::new(
                &A11Y013,
                format!(
                    "{} page(s) hide their only image from assistive technology",
                    hidden_pages.len()
                ),
                util::summarize_list(&hidden_pages, 8),
                "alt=\"\", role=\"presentation\" and aria-hidden all mean decorative; on a page \
                 whose entire content is that image they announce an empty page",
            )
            .at(hidden_pages),
        );
    }

    if !placeholders.is_empty() {
        out.push(
            Finding::new(
                &A11Y002,
                format!("{} alt text(s) look like a placeholder", placeholders.len()),
                util::summarize_list(&placeholders, 8),
                "describe what happens on the page: who is there, what they do, and the text in \
                 the balloons; a filename or a page number passes an automated check and tells a \
                 reader nothing",
            )
            .at(placeholders),
        );
    }

    let repeated: Vec<String> = by_alt
        .iter()
        .filter(|(_, pages)| pages.len() >= 3)
        .map(|(alt, pages)| {
            format!(
                "\"{}\" on {} pages ({})",
                truncate(alt, 40),
                pages.len(),
                util::summarize_list(pages, 3)
            )
        })
        .collect();

    if !repeated.is_empty() {
        out.push(
            Finding::new(
                &A11Y003,
                format!("{} alt text(s) repeat across pages", repeated.len()),
                util::summarize_list(&repeated, 6),
                "write one description per page; the same sentence on every page is the shape \
                 alt text takes when it was filled in by a batch export",
            )
            .at(repeated),
        );
    }
}

/// Reasons an alt string is very likely not a description. Each is a pattern
/// seen in exported books, not a guess at the author's intent.
fn placeholder_reason(alt: &str, path: &str) -> Option<String> {
    let trimmed = alt.trim();
    let lower = trimmed.to_ascii_lowercase();

    if lower == util::basename(path).to_ascii_lowercase() {
        return Some("it is the file name".to_string());
    }
    if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".svg")
    {
        return Some("it is a file name".to_string());
    }

    let words: Vec<&str> = lower.split_whitespace().collect();
    let generic = [
        "image",
        "img",
        "picture",
        "photo",
        "page",
        "figure",
        "illustration",
        "cover",
        "panel",
    ];
    if words.len() <= 2
        && words.iter().all(|w| {
            let stem = w.trim_matches(|c: char| !c.is_alphanumeric());
            generic.contains(&stem) || stem.chars().all(|c| c.is_ascii_digit())
        })
    {
        return Some("it names the object instead of describing it".to_string());
    }

    // A single short token with a digit ("p012", "img_07") is a slot, not a
    // sentence. Length alone is not used as evidence: a good alt can be short
    // in Japanese, where a few characters carry a whole clause.
    if words.len() == 1
        && trimmed.chars().count() <= 8
        && trimmed.chars().any(|c| c.is_ascii_digit())
    {
        return Some("it is an identifier, not a description".to_string());
    }

    None
}

fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}...")
}

fn discovery_metadata(book: &Book, _opts: &Options, out: &mut Vec<Finding>) {
    let required = [
        (
            "schema:accessMode",
            "how the content is perceived: textual, visual, auditory",
        ),
        (
            "schema:accessibilityFeature",
            "what was done for accessibility: alternativeText, readingOrder, none",
        ),
        (
            "schema:accessibilityHazard",
            "flashing, motionSimulation, sound, or their none/unknown forms",
        ),
    ];
    let mut missing: Vec<String> = Vec::new();
    for (property, hint) in required {
        if book.package.meta_values(property).is_empty() {
            missing.push(format!("{property} ({hint})"));
        }
    }
    if !missing.is_empty() {
        out.push(
            Finding::new(
                &A11Y004,
                format!("{} required discovery propert(ies) missing", missing.len()),
                missing.join("; "),
                "add them to the package metadata; EPUB Accessibility 1.1 requires all three, \
                 and an EU storefront has nothing to display without them -- \"none\" and \
                 \"unknown\" are valid answers for an unremediated book",
            )
            .at([book.opf_path.clone()]),
        );
    }

    let mut recommended: Vec<String> = Vec::new();
    if book
        .package
        .meta_values("schema:accessModeSufficient")
        .is_empty()
    {
        recommended.push("schema:accessModeSufficient".to_string());
    }
    if book
        .package
        .meta_values("schema:accessibilitySummary")
        .is_empty()
    {
        recommended.push("schema:accessibilitySummary".to_string());
    }
    if !recommended.is_empty() {
        out.push(
            Finding::new(
                &A11Y005,
                format!(
                    "{} recommended discovery propert(ies) missing",
                    recommended.len()
                ),
                recommended.join(", "),
                "for a fixed-layout book the summary is where you say the thing the metadata \
                 cannot: that the text does not resize and reflow, and what the reader gets \
                 instead",
            )
            .at([book.opf_path.clone()]),
        );
    }
}

/// `EPUB Accessibility 1.1 - WCAG 2.2 Level AA` and nothing looser: the
/// conformance string is machine-read, so a hand-written variant is not a
/// claim anyone can act on.
fn parse_conformance(value: &str) -> Option<(String, String, String)> {
    let (left, right) = value.split_once(" - ")?;
    let epub_version = left
        .trim()
        .strip_prefix("EPUB Accessibility ")?
        .trim()
        .to_string();
    let rest = right.trim().strip_prefix("WCAG ")?.trim();
    let (wcag, level) = rest.split_once(" Level ")?;
    let level = level.trim().to_string();
    if !matches!(level.as_str(), "A" | "AA" | "AAA") {
        return None;
    }
    Some((epub_version, wcag.trim().to_string(), level))
}

fn conformance(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    let claims = book.package.meta_values("dcterms:conformsTo");
    let parsed: Vec<(String, String, String)> =
        claims.iter().filter_map(|c| parse_conformance(c)).collect();

    if claims.is_empty() {
        out.push(
            Finding::new(
                &A11Y006,
                "no accessibility conformance is declared",
                "the package carries no dcterms:conformsTo".to_string(),
                format!(
                    "once the book meets it, declare <meta property=\"dcterms:conformsTo\">EPUB \
                     Accessibility 1.1 - WCAG {} Level {}</meta>; leaving it out is a valid \
                     answer only if the book does not conform",
                    opts.wcag, opts.level
                ),
            )
            .at([book.opf_path.clone()]),
        );
        return;
    }

    if parsed.is_empty() {
        out.push(
            Finding::new(
                &A11Y006,
                "dcterms:conformsTo is not in the required form",
                format!("found: {}", claims.join(" | ")),
                "the string is parsed, not read: it has to be exactly \"EPUB Accessibility \
                 <version> - WCAG <version> Level <A|AA|AAA>\"",
            )
            .at([book.opf_path.clone()]),
        );
        return;
    }

    let level_rank = |l: &str| match l {
        "A" => 1,
        "AA" => 2,
        "AAA" => 3,
        _ => 0,
    };
    let best = parsed
        .iter()
        .max_by_key(|(_, _, level)| level_rank(level))
        .unwrap();
    if level_rank(&best.2) < level_rank(&opts.level) {
        out.push(
            Finding::new(
                &A11Y006,
                format!(
                    "conformance is declared at Level {}, below the Level {} you asked for",
                    best.2, opts.level
                ),
                format!(
                    "declared: EPUB Accessibility {} - WCAG {} Level {}",
                    best.0, best.1, best.2
                ),
                format!(
                    "either raise the book to Level {} or run with --level {} so the report \
                     matches the target you are actually publishing against",
                    opts.level,
                    best.2.to_lowercase()
                ),
            )
            .at([book.opf_path.clone()])
            .with_severity(Severity::Warn),
        );
    }

    if book.package.meta_values("a11y:certifiedBy").is_empty() {
        out.push(
            Finding::new(
                &A11Y007,
                "conformance is claimed without a11y:certifiedBy",
                "dcterms:conformsTo is present but no party is named as having verified it"
                    .to_string(),
                "add <meta property=\"a11y:certifiedBy\">your organization</meta>; a claim \
                 nobody signs is one a store cannot pass on",
            )
            .at([book.opf_path.clone()]),
        );
    }
}

fn contradictions(book: &Book, out: &mut Vec<Finding>) {
    let features: Vec<String> = book
        .package
        .meta_values("schema:accessibilityFeature")
        .iter()
        .map(|v| v.to_string())
        .collect();
    let modes: Vec<String> = book
        .package
        .meta_values("schema:accessMode")
        .iter()
        .map(|v| v.to_string())
        .collect();
    let sufficient: Vec<String> = book
        .package
        .meta_values("schema:accessModeSufficient")
        .iter()
        .map(|v| v.to_string())
        .collect();

    let image_pages: Vec<&Page> = book.pages.iter().filter(|p| p.is_image_only()).collect();
    let described = image_pages
        .iter()
        .filter(|p| {
            p.images[0]
                .alt
                .as_deref()
                .map(|a| !a.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    let undescribed = image_pages.len() - described;

    let mut issues: Vec<String> = Vec::new();

    if features.iter().any(|f| f == "alternativeText") && undescribed > 0 {
        issues.push(format!(
            "accessibilityFeature says alternativeText, but {undescribed} image-only page(s) \
             have no alt text"
        ));
    }
    if features.iter().any(|f| f == "none") && described > 0 {
        issues.push(format!(
            "accessibilityFeature says none, but {described} page(s) do carry alt text"
        ));
    }
    if !image_pages.is_empty() && !modes.is_empty() && !modes.iter().any(|m| m == "visual") {
        issues.push(format!(
            "accessMode is {} but {} page(s) are a single image",
            modes.join(", "),
            image_pages.len()
        ));
    }
    if sufficient.iter().any(|s| s.trim() == "textual") && undescribed > 0 {
        issues.push(format!(
            "accessModeSufficient says textual alone is enough, but {undescribed} image-only \
             page(s) have no text alternative"
        ));
    }

    if !issues.is_empty() {
        out.push(
            Finding::new(
                &A11Y008,
                format!("{} discovery claim(s) contradict the pages", issues.len()),
                issues.join("; "),
                "make the metadata describe the book as it is; an overstated claim is worse \
                 than an honest \"none\", because a reader picks the book on the strength of it",
            )
            .at([book.opf_path.clone()]),
        );
    }
}

fn documents(book: &Book, out: &mut Vec<Finding>) {
    let mut no_lang: Vec<String> = Vec::new();
    let mut no_title: Vec<String> = Vec::new();

    for page in &book.pages {
        if page.parse_error.is_some() {
            continue;
        }
        if page.lang.is_none() {
            no_lang.push(page_label(page));
        }
        if page
            .title
            .as_deref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true)
        {
            no_title.push(page_label(page));
        }
    }

    if !no_lang.is_empty() {
        out.push(
            Finding::new(
                &A11Y009,
                format!("{} content document(s) declare no language", no_lang.len()),
                util::summarize_list(&no_lang, 8),
                "put xml:lang and lang on the html element of every page; a screen reader picks \
                 its voice from it, and dc:language in the package does not reach the document",
            )
            .at(no_lang),
        );
    }

    if !no_title.is_empty() {
        out.push(
            Finding::new(
                &A11Y010,
                format!("{} content document(s) have no title", no_title.len()),
                util::summarize_list(&no_title, 8),
                "give each page a <title>: it is what a reading system announces when the page \
                 turns, and \"Untitled\" thirty times is how a fixed-layout book usually sounds",
            )
            .at(no_title),
        );
    }
}

fn navigation(book: &Book, out: &mut Vec<Finding>) {
    let Some(nav) = &book.nav else {
        return;
    };

    if !nav.has_page_list && book.fxl_pages().count() > 0 {
        out.push(
            Finding::new(
                &A11Y011,
                "the navigation document has no page-list",
                format!(
                    "{} carries a toc but no <nav epub:type=\"page-list\">",
                    nav.path
                ),
                "add a page-list so a reader can jump to a page by number; in a fixed-layout \
                 book the page number is the only landmark there is",
            )
            .at([nav.path.clone()]),
        );
    }

    if nav.toc_entries < 2 {
        out.push(
            Finding::new(
                &A11Y012,
                format!("the table of contents has {} entr(ies)", nav.toc_entries),
                format!("{} lists almost nothing to navigate by", nav.path),
                "list the chapters, episodes or scenes; a single \"Start\" link is a table of \
                 contents in name only",
            )
            .at([nav.path.clone()]),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_conformance_string_is_parsed_not_read() {
        assert_eq!(
            parse_conformance("EPUB Accessibility 1.1 - WCAG 2.2 Level AA"),
            Some(("1.1".to_string(), "2.2".to_string(), "AA".to_string()))
        );
        assert_eq!(
            parse_conformance("EPUB Accessibility 1.1 - WCAG 2.0 Level A"),
            Some(("1.1".to_string(), "2.0".to_string(), "A".to_string()))
        );
        // Near misses that a storefront cannot act on.
        assert_eq!(parse_conformance("WCAG 2.2 AA compliant"), None);
        assert_eq!(
            parse_conformance("EPUB Accessibility 1.1 - WCAG 2.2 AA"),
            None
        );
        assert_eq!(
            parse_conformance("EPUB Accessibility 1.1 - WCAG 2.2 Level B"),
            None
        );
    }

    #[test]
    fn placeholder_alt_text_is_recognized_by_shape() {
        assert!(placeholder_reason("p001.png", "OEBPS/images/p001.png").is_some());
        assert!(placeholder_reason("cover.jpg", "OEBPS/images/c.jpg").is_some());
        assert!(placeholder_reason("page 12", "OEBPS/images/p012.png").is_some());
        assert!(placeholder_reason("image", "OEBPS/images/p012.png").is_some());
        assert!(placeholder_reason("p012", "OEBPS/images/p012.png").is_some());

        // Real descriptions are left alone, including short ones in a
        // language that carries a clause in a few characters.
        assert!(placeholder_reason("葵が商店街を駆け抜ける。", "OEBPS/images/p001.png").is_none());
        assert!(placeholder_reason(
            "Rin closes the shutters while Aoi runs up.",
            "OEBPS/images/p001.png"
        )
        .is_none());
    }

    #[test]
    fn decorative_markings_are_all_recognized() {
        let base = crate::model::PageImage {
            path: "OEBPS/images/p001.png".to_string(),
            remote: false,
            element: "img",
            alt: None,
            aria_hidden: false,
            role: None,
            pixels: None,
            bytes: None,
        };
        assert!(!is_decorative(&base));
        assert!(is_decorative(&crate::model::PageImage {
            alt: Some(String::new()),
            ..base.clone()
        }));
        assert!(is_decorative(&crate::model::PageImage {
            aria_hidden: true,
            ..base.clone()
        }));
        assert!(is_decorative(&crate::model::PageImage {
            role: Some("presentation".to_string()),
            ..base
        }));
    }
}
