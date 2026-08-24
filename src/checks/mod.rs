//! The rules, and the knobs the caller gets over them.

pub mod access;
pub mod assets;
pub mod catalog;
pub mod layout;
pub mod package;

use crate::model::{Book, Category, Finding};

/// Budgets and expectations the checks are run against.
///
/// Nothing here is presented as a store's official limit: file-size ceilings
/// and required WCAG levels change per retailer and per year, so they are
/// arguments with documented defaults rather than baked-in claims.
#[derive(Debug, Clone)]
pub struct Options {
    /// Ceiling for a single image resource, in bytes.
    pub max_image_bytes: u64,
    /// Ceiling for the whole publication, uncompressed, in bytes.
    pub max_total_bytes: u64,
    /// Smallest image-pixels-to-viewport-pixels ratio that is not reported as
    /// upscaling.
    pub min_scale: f64,
    /// Allowed difference between an image's aspect ratio and its viewport's,
    /// as a percentage.
    pub aspect_tolerance: f64,
    /// WCAG version expected in the conformance string, e.g. "2.2".
    pub wcag: String,
    /// WCAG level expected in the conformance string, e.g. "AA".
    pub level: String,
    /// When non-empty, only these check ids or categories run.
    pub only: Vec<String>,
    /// These check ids or categories never run.
    pub skip: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            // 5 MiB per image is a deliberately loose default: it catches the
            // 40 MB scan someone forgot to downsample without pretending to
            // know a given store's rule.
            max_image_bytes: 5 * 1024 * 1024,
            max_total_bytes: u64::MAX,
            min_scale: 1.0,
            aspect_tolerance: 2.0,
            wcag: "2.2".to_string(),
            level: "AA".to_string(),
            only: Vec::new(),
            skip: Vec::new(),
        }
    }
}

impl Options {
    fn selected(&self, id: &str, category: Category) -> bool {
        let matches = |pattern: &str| -> bool {
            let p = pattern.trim();
            if p.is_empty() {
                return false;
            }
            if p.eq_ignore_ascii_case(id) {
                return true;
            }
            if Category::parse(p) == Some(category) {
                return true;
            }
            // A prefix selects a family: "FXL" is every layout rule, "A11Y"
            // every accessibility rule.
            id.to_ascii_uppercase().starts_with(&p.to_ascii_uppercase())
        };

        if self.skip.iter().any(|p| matches(p)) {
            return false;
        }
        if self.only.is_empty() {
            return true;
        }
        self.only.iter().any(|p| matches(p))
    }
}

/// Run every selected check and return the findings, worst first, then by id.
pub fn run(book: &Book, opts: &Options) -> Vec<Finding> {
    let mut findings = Vec::new();
    package::check(book, opts, &mut findings);
    layout::check(book, opts, &mut findings);
    assets::check(book, opts, &mut findings);
    access::check(book, opts, &mut findings);

    findings.retain(|f| opts.selected(f.id, f.category));
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.id.cmp(b.id))
    });
    findings
}

/// `path (page N)`, the label every finding uses to point at a spine item.
pub(crate) fn page_label(page: &crate::model::Page) -> String {
    format!("{} (page {})", page.path, page.index)
}
