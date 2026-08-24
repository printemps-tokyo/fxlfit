//! The data the checks run against, and the findings they produce.
//!
//! Everything here is a plain description of what was read out of the EPUB.
//! No check logic lives in this module, so a parser fix can never quietly
//! change a verdict.

use std::collections::BTreeMap;
use std::fmt;

/// How badly a finding blocks the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Worth knowing, never a reason to hold a release.
    Info,
    /// The book will ship, but a reader or a store will notice.
    Warn,
    /// The book is broken or will be rejected, in the judgement of this tool.
    Error,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
        }
    }

    pub fn parse(s: &str) -> Option<Severity> {
        match s.to_ascii_lowercase().as_str() {
            "error" => Some(Severity::Error),
            "warn" | "warning" => Some(Severity::Warn),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Which part of the book a finding is about. Also selectable on the command
/// line, so the names are short and lowercase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    /// Package, container, navigation, identifiers.
    Package,
    /// Fixed-layout declarations: layout, viewport, spread, progression.
    Layout,
    /// Images and the size budget the book spends on them.
    Assets,
    /// EPUB Accessibility 1.1 content and discovery metadata.
    Access,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Package => "package",
            Category::Layout => "layout",
            Category::Assets => "assets",
            Category::Access => "access",
        }
    }

    pub fn parse(s: &str) -> Option<Category> {
        match s.to_ascii_lowercase().as_str() {
            "package" | "pkg" => Some(Category::Package),
            "layout" | "fxl" => Some(Category::Layout),
            "assets" | "asset" | "img" => Some(Category::Assets),
            "access" | "a11y" | "accessibility" => Some(Category::Access),
            _ => None,
        }
    }
}

/// One thing the tool has to say about the book.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Stable identifier, e.g. `FXL003`. Used by `--only` / `--skip` and by
    /// anything downstream that pins expectations, so it never gets reused
    /// for a different meaning.
    pub id: &'static str,
    pub severity: Severity,
    pub category: Category,
    /// One line, no trailing period, states what is wrong.
    pub title: String,
    /// The evidence: what was found, where, how many times.
    pub detail: String,
    /// Paths inside the EPUB the finding points at.
    pub locations: Vec<String>,
    /// What to change. Concrete, not "consider improving".
    pub fix: String,
    /// Primary source for the rule.
    pub reference: &'static str,
}

impl Finding {
    pub fn new(
        spec: &CheckSpec,
        title: impl Into<String>,
        detail: impl Into<String>,
        fix: impl Into<String>,
    ) -> Finding {
        Finding {
            id: spec.id,
            severity: spec.severity,
            category: spec.category,
            title: title.into(),
            detail: detail.into(),
            locations: Vec::new(),
            fix: fix.into(),
            reference: spec.reference,
        }
    }

    pub fn at(mut self, locations: impl IntoIterator<Item = String>) -> Finding {
        self.locations = locations.into_iter().collect();
        self
    }

    /// Lower the severity of a finding whose default rank does not fit the
    /// evidence (e.g. a rule that is an error for image-only pages and a
    /// warning elsewhere).
    pub fn with_severity(mut self, severity: Severity) -> Finding {
        self.severity = severity;
        self
    }
}

/// The catalogue entry for a check: identity and default rank, declared once
/// so `--list-checks` and the checks themselves cannot drift apart.
#[derive(Debug, Clone, Copy)]
pub struct CheckSpec {
    pub id: &'static str,
    pub severity: Severity,
    pub category: Category,
    /// One line, printed by `--list-checks`.
    pub summary: &'static str,
    pub reference: &'static str,
}

/// Whether a spine item is laid out by the reading system or by the author.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Reflowable,
    PrePaginated,
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Layout::Reflowable => "reflowable",
            Layout::PrePaginated => "pre-paginated",
        }
    }
}

/// A `<meta>` in the package document, in the EPUB 3 form (`property`) or the
/// EPUB 2 form (`name`/`content`) that plenty of production files still carry.
#[derive(Debug, Clone, Default)]
pub struct MetaEntry {
    pub property: Option<String>,
    pub name: Option<String>,
    pub content: Option<String>,
    pub refines: Option<String>,
    pub id: Option<String>,
    pub scheme: Option<String>,
    pub text: String,
}

impl MetaEntry {
    /// The value a consumer would read: element text for EPUB 3 metadata,
    /// the `content` attribute for the EPUB 2 form.
    pub fn value(&self) -> &str {
        if !self.text.trim().is_empty() {
            self.text.trim()
        } else {
            self.content.as_deref().unwrap_or("").trim()
        }
    }
}

/// A Dublin Core element of the package metadata.
#[derive(Debug, Clone)]
pub struct DcEntry {
    /// Local name without the `dc:` prefix, e.g. `title`, `language`.
    pub name: String,
    pub value: String,
    pub id: Option<String>,
}

/// A manifest entry, with `href` already resolved to a path inside the ZIP.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    /// Raw `href` as authored, kept for error messages.
    pub href_raw: String,
    /// Resolved, percent-decoded, normalized path inside the container.
    pub path: String,
    pub media_type: String,
    pub properties: Vec<String>,
    pub fallback: Option<String>,
    /// True when the href is an absolute URL rather than a container path.
    pub remote: bool,
}

impl Item {
    pub fn has_property(&self, p: &str) -> bool {
        self.properties.iter().any(|x| x == p)
    }
}

/// One `<itemref>` of the spine, in reading order.
#[derive(Debug, Clone)]
pub struct SpineRef {
    pub idref: String,
    pub linear: bool,
    pub properties: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Spine {
    pub page_progression_direction: Option<String>,
    pub toc: Option<String>,
    pub refs: Vec<SpineRef>,
}

#[derive(Debug, Clone, Default)]
pub struct Package {
    pub version: Option<String>,
    pub unique_identifier: Option<String>,
    pub dc: Vec<DcEntry>,
    pub meta: Vec<MetaEntry>,
    pub items: Vec<Item>,
    pub spine: Spine,
    /// `prefix` attribute of the package element, if any.
    pub prefix: Option<String>,
}

impl Package {
    pub fn dc_values(&self, name: &str) -> Vec<&str> {
        self.dc
            .iter()
            .filter(|e| e.name == name)
            .map(|e| e.value.as_str())
            .filter(|v| !v.trim().is_empty())
            .collect()
    }

    /// Values of a package-level `meta property="..."`, ignoring refinements
    /// of other elements (a refined meta describes its target, not the book).
    pub fn meta_values(&self, property: &str) -> Vec<&str> {
        self.meta
            .iter()
            .filter(|m| m.refines.is_none())
            .filter(|m| {
                m.property.as_deref() == Some(property) || m.name.as_deref() == Some(property)
            })
            .map(|m| m.value())
            .filter(|v| !v.is_empty())
            .collect()
    }

    pub fn item_by_id(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_by_path(&self, path: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.path == path)
    }
}

/// An image reference found inside a content document.
#[derive(Debug, Clone)]
pub struct PageImage {
    /// Path inside the container, or the raw URL when remote.
    pub path: String,
    pub remote: bool,
    /// `<img>`, `<image>` (SVG) or `<object>`.
    pub element: &'static str,
    /// `alt` for HTML, `<title>`/`aria-label` for SVG. `None` means the
    /// attribute was absent, `Some("")` means it was explicitly empty.
    pub alt: Option<String>,
    pub aria_hidden: bool,
    pub role: Option<String>,
    /// Intrinsic pixel size, when the bytes could be read and sniffed.
    pub pixels: Option<(u32, u32)>,
    /// Uncompressed size of the resource in the container.
    pub bytes: Option<u64>,
}

/// A spine item, resolved: what it declares and what it actually contains.
#[derive(Debug, Clone)]
pub struct Page {
    /// Position in the spine, 1-based, as a human would count pages.
    pub index: usize,
    pub idref: String,
    pub path: String,
    pub media_type: String,
    pub linear: bool,
    pub spine_properties: Vec<String>,
    pub layout: Layout,
    /// Viewport in CSS pixels and where it came from.
    pub viewport: Option<(u32, u32)>,
    pub viewport_source: ViewportSource,
    /// Raw viewport declaration, for reporting a malformed one back.
    pub viewport_raw: Option<String>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub images: Vec<PageImage>,
    /// Length of the rendered text of the document, whitespace collapsed.
    pub text_len: usize,
    /// Set when the document could not be parsed or read at all.
    pub parse_error: Option<String>,
}

impl Page {
    pub fn has_spine_property(&self, p: &str) -> bool {
        self.spine_properties.iter().any(|x| x == p)
    }

    /// A page whose entire content is one image: the normal shape of a comic
    /// or picture-book page, and the shape most accessibility rules are about.
    pub fn is_image_only(&self) -> bool {
        self.images.len() == 1 && self.text_len < 3
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewportSource {
    /// `<meta name="viewport" content="width=..., height=...">`
    MetaTag,
    /// `width`/`height` or `viewBox` of an SVG spine item.
    SvgAttributes,
    #[default]
    None,
}

impl ViewportSource {
    pub fn label(self) -> &'static str {
        match self {
            ViewportSource::MetaTag => "meta",
            ViewportSource::SvgAttributes => "svg",
            ViewportSource::None => "none",
        }
    }
}

/// A resource inside the container, whether or not the package declares it.
#[derive(Debug, Clone)]
pub struct Resource {
    pub path: String,
    /// Uncompressed size in bytes.
    pub bytes: u64,
    /// Compressed size in bytes, as stored.
    pub stored_bytes: u64,
}

/// Navigation document facts the checks care about.
#[derive(Debug, Clone, Default)]
pub struct Nav {
    pub path: String,
    pub toc_entries: usize,
    pub has_page_list: bool,
    pub has_landmarks: bool,
}

/// Everything read out of one EPUB file.
#[derive(Debug)]
pub struct Book {
    pub file: String,
    pub opf_path: String,
    pub package: Package,
    pub pages: Vec<Page>,
    pub nav: Option<Nav>,
    /// Every entry in the container, keyed by path.
    pub resources: BTreeMap<String, Resource>,
    /// Problems hit while reading, that are not check results themselves.
    pub read_warnings: Vec<String>,
    /// True when `META-INF/encryption.xml` is present.
    pub encrypted: bool,
    /// Total uncompressed size of the container, in bytes.
    pub total_bytes: u64,
    /// Size of the EPUB file on disk, in bytes.
    pub file_bytes: u64,
    /// True when the package does not declare pre-paginated layout but the
    /// pages are unmistakably fixed-layout, so the layout checks were run
    /// against them anyway. The missing declaration is still reported.
    pub layout_inferred: bool,
}

impl Book {
    /// Spine pages that are laid out by the author. Everything fixed-layout
    /// is judged against these and nothing else.
    pub fn fxl_pages(&self) -> impl Iterator<Item = &Page> {
        self.pages
            .iter()
            .filter(|p| p.layout == Layout::PrePaginated)
    }

    pub fn declared_layout(&self) -> Option<Layout> {
        match self
            .package
            .meta_values("rendition:layout")
            .first()
            .copied()
        {
            Some("pre-paginated") => Some(Layout::PrePaginated),
            Some("reflowable") => Some(Layout::Reflowable),
            _ => None,
        }
    }
}
