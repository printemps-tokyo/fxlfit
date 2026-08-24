//! Rendering the report: the human one, the per-page table and the JSON.
//!
//! The text report is written for someone about to upload a book, so it leads
//! with what the book is, then what is wrong with it, and ends with a verdict
//! and the exit code that goes with it. The JSON carries the same facts with
//! no formatting decisions in it.

use std::io::IsTerminal;

use crate::checks::Options;
use crate::model::{Book, Category, Finding, Layout, Severity, ViewportSource};
use crate::util;

pub struct Style {
    pub color: bool,
}

impl Style {
    pub fn new(no_color: bool) -> Style {
        let enabled =
            !no_color && std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal();
        Style { color: enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    fn severity(&self, severity: Severity) -> String {
        let code = match severity {
            Severity::Error => "31;1",
            Severity::Warn => "33",
            Severity::Info => "36",
        };
        self.paint(code, severity.label())
    }

    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }

    fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    ReadyWithNotes,
    NotReady,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Ready => "READY",
            Verdict::ReadyWithNotes => "READY WITH NOTES",
            Verdict::NotReady => "NOT READY",
        }
    }
}

pub fn verdict(findings: &[Finding]) -> Verdict {
    if findings.iter().any(|f| f.severity == Severity::Error) {
        Verdict::NotReady
    } else if findings.iter().any(|f| f.severity == Severity::Warn) {
        Verdict::ReadyWithNotes
    } else {
        Verdict::Ready
    }
}

pub fn count(findings: &[Finding], severity: Severity) -> usize {
    findings.iter().filter(|f| f.severity == severity).count()
}

/// The book as the checks saw it: the header of the text report, and the part
/// that most often explains a finding on its own.
fn overview(book: &Book) -> Vec<(String, String)> {
    let pkg = &book.package;
    let mut rows = Vec::new();

    rows.push((
        "book".to_string(),
        format!(
            "{} ({} on disk, {} uncompressed)",
            book.file,
            util::human_bytes(book.file_bytes),
            util::human_bytes(book.total_bytes)
        ),
    ));
    rows.push((
        "package".to_string(),
        format!(
            "{} (EPUB {})",
            book.opf_path,
            pkg.version.clone().unwrap_or_else(|| "?".to_string())
        ),
    ));
    rows.push((
        "title".to_string(),
        format!(
            "{} [{}]",
            pkg.dc_values("title").first().copied().unwrap_or("(none)"),
            pkg.dc_values("language").join(", ")
        ),
    ));

    let fxl = book.fxl_pages().count();
    let viewports: Vec<(u32, u32)> = {
        let mut v: Vec<(u32, u32)> = book.fxl_pages().filter_map(|p| p.viewport).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let viewport_text = match viewports.len() {
        0 => "no viewport declared".to_string(),
        1 => format!("viewport {}x{}", viewports[0].0, viewports[0].1),
        n => format!("{n} distinct viewports"),
    };
    rows.push((
        "layout".to_string(),
        format!(
            "{}, {fxl} of {} spine page(s) pre-paginated, {viewport_text}",
            book.declared_layout()
                .map(|l| l.label().to_string())
                .unwrap_or_else(|| {
                    if book.layout_inferred {
                        "not declared (pages read as fixed-layout anyway)".to_string()
                    } else {
                        "not declared".to_string()
                    }
                }),
            book.pages.len()
        ),
    ));
    rows.push((
        "spread".to_string(),
        format!(
            "rendition:spread {}, page progression {}",
            first_or(&pkg.meta_values("rendition:spread"), "auto (default)"),
            pkg.spine
                .page_progression_direction
                .clone()
                .unwrap_or_else(|| "ltr (default)".to_string())
        ),
    ));

    let images: usize = book.pages.iter().map(|p| p.images.len()).sum();
    let with_alt: usize = book
        .pages
        .iter()
        .flat_map(|p| p.images.iter())
        .filter(|i| {
            i.alt
                .as_deref()
                .map(|a| !a.trim().is_empty())
                .unwrap_or(false)
        })
        .count();
    rows.push((
        "images".to_string(),
        format!("{images} referenced, {with_alt} with a text alternative"),
    ));
    rows.push((
        "a11y metadata".to_string(),
        format!(
            "conformsTo {}, certifiedBy {}",
            first_or(&pkg.meta_values("dcterms:conformsTo"), "(none)"),
            first_or(&pkg.meta_values("a11y:certifiedBy"), "(none)")
        ),
    ));

    rows
}

fn first_or(values: &[&str], fallback: &str) -> String {
    values
        .first()
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback.to_string())
}

pub fn render_text(book: &Book, findings: &[Finding], opts: &Options, style: &Style) -> String {
    let mut out = String::new();
    out.push_str(&style.bold("fxlfit: fixed-layout preflight\n"));

    for (key, value) in overview(book) {
        out.push_str(&format!("{:<14} {value}\n", format!("{key}:")));
    }
    out.push_str(&format!(
        "{:<14} image {}, total {}, min-scale {:.2}x, aspect tolerance {:.1}%, target WCAG {} Level {}\n",
        "budgets:",
        util::human_bytes(opts.max_image_bytes),
        util::human_bytes(opts.max_total_bytes),
        opts.min_scale,
        opts.aspect_tolerance,
        opts.wcag,
        opts.level
    ));

    if !book.read_warnings.is_empty() {
        out.push('\n');
        out.push_str("while reading:\n");
        for w in &book.read_warnings {
            out.push_str(&format!("  ! {w}\n"));
        }
    }

    let errors = count(findings, Severity::Error);
    let warns = count(findings, Severity::Warn);
    let infos = count(findings, Severity::Info);

    out.push('\n');
    if findings.is_empty() {
        out.push_str("no findings\n");
    } else {
        out.push_str(&format!(
            "findings: {errors} error(s), {warns} warning(s), {infos} note(s)\n\n"
        ));
        for finding in findings {
            out.push_str(&format!(
                "{:<6} {:<8} {}\n",
                style.severity(finding.severity),
                style.bold(finding.id),
                finding.title
            ));
            if !finding.detail.trim().is_empty() {
                out.push_str(&format!("       {:<8} {}\n", "", finding.detail));
            }
            out.push_str(&format!(
                "       {:<8} {} {}\n",
                "",
                style.dim("fix:"),
                finding.fix
            ));
            out.push_str(&format!(
                "       {:<8} {} {}\n\n",
                "",
                style.dim("ref:"),
                style.dim(finding.reference)
            ));
        }
    }

    let v = verdict(findings);
    let painted = match v {
        Verdict::Ready => style.paint("32;1", v.label()),
        Verdict::ReadyWithNotes => style.paint("33;1", v.label()),
        Verdict::NotReady => style.paint("31;1", v.label()),
    };
    out.push_str(&format!(
        "verdict: {painted} ({errors} error(s), {warns} warning(s), {infos} note(s))\n"
    ));
    out.push_str(&style.dim(
        "fxlfit is a preflight, not a validator: run epubcheck for conformance, and read the \
         pages yourself for whether the alt text is any good.\n",
    ));
    out
}

/// The per-page table, printed by `--pages`. One line per spine item, in
/// reading order, so a book can be scanned for the page that is different.
pub fn render_pages(book: &Book, style: &Style) -> String {
    let mut out = String::new();
    out.push_str(&style.bold("pages\n"));
    out.push_str(&format!(
        "{:>4}  {:<34} {:<10} {:<11} {:<30} {:<11} {:>9}  {}\n",
        "#", "document", "layout", "viewport", "image", "pixels", "bytes", "alt"
    ));

    for page in &book.pages {
        let viewport = match (page.viewport, page.viewport_source) {
            (Some((w, h)), _) => format!("{w}x{h}"),
            (None, ViewportSource::None) => "-".to_string(),
            (None, _) => "unparsed".to_string(),
        };
        let (image, pixels, bytes) = match page.images.first() {
            Some(img) => (
                util::basename(&img.path).to_string(),
                img.pixels
                    .map(|(w, h)| format!("{w}x{h}"))
                    .unwrap_or_else(|| "-".to_string()),
                img.bytes
                    .map(util::human_bytes)
                    .unwrap_or_else(|| "-".to_string()),
            ),
            None => ("-".to_string(), "-".to_string(), "-".to_string()),
        };
        let alt = match page.images.first().and_then(|i| i.alt.as_deref()) {
            Some("") => "empty",
            Some(_) => "yes",
            None if page.images.is_empty() => "-",
            None => "MISSING",
        };
        let extra = if page.images.len() > 1 {
            format!(" (+{} more)", page.images.len() - 1)
        } else {
            String::new()
        };

        out.push_str(&format!(
            "{:>4}  {:<34} {:<10} {:<11} {:<30} {:<11} {:>9}  {}{}\n",
            page.index,
            elide(&page.path, 34),
            if page.layout == Layout::PrePaginated {
                "fixed"
            } else {
                "reflow"
            },
            viewport,
            elide(&image, 30),
            pixels,
            bytes,
            alt,
            extra
        ));
    }
    out.push('\n');
    out
}

fn elide(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(s.chars().count() - (max - 3)).collect();
    format!("...{tail}")
}

// --- JSON -------------------------------------------------------------------

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", escape(s))
}

fn list(items: &[String]) -> String {
    format!("[{}]", items.join(","))
}

pub fn render_json(book: &Book, findings: &[Finding], opts: &Options) -> String {
    let errors = count(findings, Severity::Error);
    let warns = count(findings, Severity::Warn);
    let infos = count(findings, Severity::Info);

    let findings_json: Vec<String> = findings
        .iter()
        .map(|f| {
            format!(
                "{{\"id\":{},\"severity\":{},\"category\":{},\"title\":{},\"detail\":{},\"fix\":{},\"reference\":{},\"locations\":{}}}",
                quoted(f.id),
                quoted(f.severity.label()),
                quoted(f.category.label()),
                quoted(&f.title),
                quoted(&f.detail),
                quoted(&f.fix),
                quoted(f.reference),
                list(&f.locations.iter().map(|l| quoted(l)).collect::<Vec<_>>())
            )
        })
        .collect();

    let pages_json: Vec<String> = book
        .pages
        .iter()
        .map(|p| {
            let images: Vec<String> = p
                .images
                .iter()
                .map(|i| {
                    format!(
                        "{{\"path\":{},\"remote\":{},\"element\":{},\"alt\":{},\"ariaHidden\":{},\"pixels\":{},\"bytes\":{}}}",
                        quoted(&i.path),
                        i.remote,
                        quoted(i.element),
                        i.alt.as_deref().map(quoted).unwrap_or_else(|| "null".to_string()),
                        i.aria_hidden,
                        i.pixels
                            .map(|(w, h)| format!("[{w},{h}]"))
                            .unwrap_or_else(|| "null".to_string()),
                        i.bytes.map(|b| b.to_string()).unwrap_or_else(|| "null".to_string())
                    )
                })
                .collect();
            format!(
                "{{\"index\":{},\"path\":{},\"layout\":{},\"linear\":{},\"viewport\":{},\"viewportSource\":{},\"title\":{},\"language\":{},\"textLength\":{},\"spineProperties\":{},\"images\":{}}}",
                p.index,
                quoted(&p.path),
                quoted(p.layout.label()),
                p.linear,
                p.viewport
                    .map(|(w, h)| format!("[{w},{h}]"))
                    .unwrap_or_else(|| "null".to_string()),
                quoted(p.viewport_source.label()),
                p.title.as_deref().map(quoted).unwrap_or_else(|| "null".to_string()),
                p.lang.as_deref().map(quoted).unwrap_or_else(|| "null".to_string()),
                p.text_len,
                list(&p.spine_properties.iter().map(|s| quoted(s)).collect::<Vec<_>>()),
                list(&images)
            )
        })
        .collect();

    let categories: Vec<String> = [
        Category::Package,
        Category::Layout,
        Category::Assets,
        Category::Access,
    ]
    .iter()
    .map(|c| {
        format!(
            "{}:{}",
            quoted(c.label()),
            findings.iter().filter(|f| f.category == *c).count()
        )
    })
    .collect();

    format!(
        "{{\"tool\":\"fxlfit\",\"version\":{},\"file\":{},\"package\":{},\"epubVersion\":{},\
\"summary\":{{\"verdict\":{},\"errors\":{},\"warnings\":{},\"notes\":{},\"byCategory\":{{{}}}}},\
\"budgets\":{{\"maxImageBytes\":{},\"maxTotalBytes\":{},\"minScale\":{},\"aspectTolerancePercent\":{},\"wcag\":{},\"level\":{}}},\
\"book\":{{\"fileBytes\":{},\"uncompressedBytes\":{},\"declaredLayout\":{},\"pageProgressionDirection\":{},\"spineLength\":{},\"prePaginatedPages\":{},\"encrypted\":{}}},\
\"pages\":{},\"findings\":{},\"readWarnings\":{}}}\n",
        quoted(env!("CARGO_PKG_VERSION")),
        quoted(&book.file),
        quoted(&book.opf_path),
        book.package.version.as_deref().map(quoted).unwrap_or_else(|| "null".to_string()),
        quoted(verdict(findings).label()),
        errors,
        warns,
        infos,
        categories.join(","),
        if opts.max_image_bytes == u64::MAX { "null".to_string() } else { opts.max_image_bytes.to_string() },
        if opts.max_total_bytes == u64::MAX { "null".to_string() } else { opts.max_total_bytes.to_string() },
        opts.min_scale,
        opts.aspect_tolerance,
        quoted(&opts.wcag),
        quoted(&opts.level),
        book.file_bytes,
        book.total_bytes,
        book.declared_layout().map(|l| quoted(l.label())).unwrap_or_else(|| "null".to_string()),
        book.package
            .spine
            .page_progression_direction
            .as_deref()
            .map(quoted)
            .unwrap_or_else(|| "null".to_string()),
        book.pages.len(),
        book.fxl_pages().count(),
        book.encrypted,
        list(&pages_json),
        list(&findings_json),
        list(&book.read_warnings.iter().map(|w| quoted(w)).collect::<Vec<_>>())
    )
}

/// The check catalogue, for `--list-checks`.
pub fn render_catalog(style: &Style) -> String {
    let mut out = String::new();
    out.push_str(&style.bold("checks\n"));
    out.push_str(&format!(
        "{:<9} {:<6} {:<8} {}\n",
        "id", "sev", "category", "what it reports"
    ));
    for spec in crate::checks::catalog::ALL {
        out.push_str(&format!(
            "{:<9} {:<6} {:<8} {}\n",
            spec.id,
            spec.severity.label(),
            spec.category.label(),
            spec.summary
        ));
    }
    out.push_str(&style.dim(
        "\nselect with --only / --skip: an id (FXL003), a family prefix (A11Y) or a category \
         (layout, access, assets, package).\n",
    ));
    out
}
