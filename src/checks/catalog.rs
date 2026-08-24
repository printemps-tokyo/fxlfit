//! The check catalogue.
//!
//! Every rule is declared once, here, with the primary source it comes from.
//! `--list-checks` prints this table, `--only` / `--skip` select from it, and
//! the check functions borrow their identity from it, so a rule cannot exist
//! without a reference or drift away from the id it was released under.
//!
//! Sources were read on 2026-08-24:
//!   EPUB 3.3                        https://www.w3.org/TR/epub-33/
//!   EPUB Accessibility 1.1          https://www.w3.org/TR/epub-a11y-11/
//!   EPUB Accessibility - EAA        https://www.w3.org/TR/epub-a11y-eaa-mapping/
//!   Fixed-Layout Accessibility (WG note)
//!                                   https://w3c.github.io/epub-specs/wg-notes/fxl-a11y-tech/

use crate::model::{Category, CheckSpec, Severity};

const EPUB33_FXL: &str = "https://www.w3.org/TR/epub-33/#sec-fixed-layouts";
const EPUB33_PKG: &str = "https://www.w3.org/TR/epub-33/#sec-pkg-doc";
const EPUB33_NAV: &str = "https://www.w3.org/TR/epub-33/#sec-nav";
const EPUB33_SPINE: &str = "https://www.w3.org/TR/epub-33/#sec-spine-elem";
const EPUB33_RESOURCES: &str = "https://www.w3.org/TR/epub-33/#sec-resource-locations";
const A11Y_DISCOVERY: &str = "https://www.w3.org/TR/epub-a11y-11/#sec-discovery";
const A11Y_CONFORMANCE: &str = "https://www.w3.org/TR/epub-a11y-11/#sec-conf-reporting";
const A11Y_CONTENT: &str = "https://www.w3.org/TR/epub-a11y-11/#sec-conf-content";
const FXL_A11Y_NOTE: &str = "https://w3c.github.io/epub-specs/wg-notes/fxl-a11y-tech/";
const EAA_MAPPING: &str = "https://www.w3.org/TR/epub-a11y-eaa-mapping/";

macro_rules! spec {
    ($konst:ident, $id:literal, $sev:ident, $cat:ident, $summary:literal, $reference:expr) => {
        pub const $konst: CheckSpec = CheckSpec {
            id: $id,
            severity: Severity::$sev,
            category: Category::$cat,
            summary: $summary,
            reference: $reference,
        };
    };
}

// --- package ---------------------------------------------------------------

spec!(
    PKG001,
    "PKG001",
    Error,
    Package,
    "dc:title, dc:language or dc:identifier is missing",
    EPUB33_PKG
);
spec!(
    PKG002,
    "PKG002",
    Error,
    Package,
    "dcterms:modified is missing from the package metadata",
    EPUB33_PKG
);
spec!(
    PKG003,
    "PKG003",
    Error,
    Package,
    "no navigation document is declared in the manifest",
    EPUB33_NAV
);
spec!(
    PKG004,
    "PKG004",
    Warn,
    Package,
    "no cover image is declared",
    EPUB33_PKG
);
spec!(
    PKG005,
    "PKG005",
    Error,
    Package,
    "a manifest item is not present in the container",
    EPUB33_PKG
);
spec!(
    PKG006,
    "PKG006",
    Warn,
    Package,
    "container carries resources nothing references",
    EPUB33_PKG
);
spec!(
    PKG007,
    "PKG007",
    Error,
    Package,
    "a page depends on a remote resource",
    EPUB33_RESOURCES
);
spec!(
    PKG008,
    "PKG008",
    Warn,
    Package,
    "the container is encrypted or obfuscated",
    EPUB33_PKG
);
spec!(
    PKG009,
    "PKG009",
    Error,
    Package,
    "the spine is empty or points at missing items",
    EPUB33_SPINE
);
spec!(
    PKG010,
    "PKG010",
    Error,
    Package,
    "a content document could not be parsed",
    EPUB33_PKG
);
spec!(
    PKG011,
    "PKG011",
    Info,
    Package,
    "the package is not EPUB 3",
    EPUB33_PKG
);

// --- fixed layout ----------------------------------------------------------

spec!(
    FXL001,
    "FXL001",
    Error,
    Layout,
    "rendition:layout pre-paginated is not declared",
    EPUB33_FXL
);
spec!(
    FXL002,
    "FXL002",
    Warn,
    Layout,
    "the spine mixes pre-paginated and reflowable pages",
    EPUB33_FXL
);
spec!(
    FXL003,
    "FXL003",
    Error,
    Layout,
    "a fixed-layout page declares no usable viewport",
    EPUB33_FXL
);
spec!(
    FXL004,
    "FXL004",
    Warn,
    Layout,
    "viewport dimensions are not consistent across pages",
    EPUB33_FXL
);
spec!(
    FXL005,
    "FXL005",
    Warn,
    Layout,
    "a page image does not match the aspect ratio of its viewport",
    EPUB33_FXL
);
spec!(
    FXL006,
    "FXL006",
    Warn,
    Layout,
    "rendition:spread is not declared",
    EPUB33_FXL
);
spec!(
    FXL007,
    "FXL007",
    Warn,
    Layout,
    "page-spread properties are incomplete or do not alternate",
    EPUB33_FXL
);
spec!(
    FXL008,
    "FXL008",
    Warn,
    Layout,
    "page-progression-direction is not declared",
    EPUB33_SPINE
);
spec!(
    FXL009,
    "FXL009",
    Warn,
    Layout,
    "rendition:flow is declared on a pre-paginated publication",
    EPUB33_FXL
);
spec!(
    FXL010,
    "FXL010",
    Warn,
    Layout,
    "the deprecated rendition:viewport property is used",
    EPUB33_FXL
);
spec!(
    FXL011,
    "FXL011",
    Error,
    Layout,
    "a viewport is declared without pixel dimensions",
    EPUB33_FXL
);

// --- assets ----------------------------------------------------------------

spec!(
    IMG001,
    "IMG001",
    Error,
    Assets,
    "a referenced image is not in the container",
    EPUB33_PKG
);
spec!(
    IMG002,
    "IMG002",
    Warn,
    Assets,
    "an image is over the per-image budget",
    EPUB33_PKG
);
spec!(
    IMG003,
    "IMG003",
    Warn,
    Assets,
    "a page image is smaller than its viewport and will be upscaled",
    FXL_A11Y_NOTE
);
spec!(
    IMG004,
    "IMG004",
    Info,
    Assets,
    "a page image is far larger than its viewport",
    EPUB33_FXL
);
spec!(
    IMG005,
    "IMG005",
    Warn,
    Assets,
    "the publication is over the total size budget",
    EPUB33_PKG
);
spec!(
    IMG006,
    "IMG006",
    Warn,
    Assets,
    "an image uses a format outside the EPUB 3 core media types",
    EPUB33_RESOURCES
);
spec!(
    IMG007,
    "IMG007",
    Warn,
    Assets,
    "a manifest media-type disagrees with the bytes",
    EPUB33_PKG
);
spec!(
    IMG008,
    "IMG008",
    Warn,
    Assets,
    "page images are authored at inconsistent resolutions",
    EPUB33_FXL
);

// --- accessibility ---------------------------------------------------------

spec!(
    A11Y001,
    "A11Y001",
    Error,
    Access,
    "a page image carries no text alternative",
    A11Y_CONTENT
);
spec!(
    A11Y002,
    "A11Y002",
    Warn,
    Access,
    "alt text looks like a filename or a placeholder",
    FXL_A11Y_NOTE
);
spec!(
    A11Y003,
    "A11Y003",
    Warn,
    Access,
    "the same alt text is repeated across pages",
    FXL_A11Y_NOTE
);
spec!(
    A11Y004,
    "A11Y004",
    Error,
    Access,
    "required discovery metadata is missing",
    A11Y_DISCOVERY
);
spec!(
    A11Y005,
    "A11Y005",
    Warn,
    Access,
    "recommended discovery metadata is missing",
    A11Y_DISCOVERY
);
spec!(
    A11Y006,
    "A11Y006",
    Warn,
    Access,
    "dcterms:conformsTo is missing or not in the required form",
    A11Y_CONFORMANCE
);
spec!(
    A11Y007,
    "A11Y007",
    Warn,
    Access,
    "conformance is claimed without a11y:certifiedBy",
    A11Y_CONFORMANCE
);
spec!(
    A11Y008,
    "A11Y008",
    Warn,
    Access,
    "discovery metadata contradicts the content",
    EAA_MAPPING
);
spec!(
    A11Y009,
    "A11Y009",
    Warn,
    Access,
    "a content document declares no language",
    A11Y_CONTENT
);
spec!(
    A11Y010,
    "A11Y010",
    Warn,
    Access,
    "a content document has no title",
    FXL_A11Y_NOTE
);
spec!(
    A11Y011,
    "A11Y011",
    Warn,
    Access,
    "the navigation document has no page-list",
    A11Y_DISCOVERY
);
spec!(
    A11Y012,
    "A11Y012",
    Warn,
    Access,
    "the table of contents is too thin to navigate by",
    EPUB33_NAV
);
spec!(
    A11Y013,
    "A11Y013",
    Error,
    Access,
    "a page hides its only image from assistive technology",
    FXL_A11Y_NOTE
);

pub const ALL: &[CheckSpec] = &[
    PKG001, PKG002, PKG003, PKG004, PKG005, PKG006, PKG007, PKG008, PKG009, PKG010, PKG011, FXL001,
    FXL002, FXL003, FXL004, FXL005, FXL006, FXL007, FXL008, FXL009, FXL010, FXL011, IMG001, IMG002,
    IMG003, IMG004, IMG005, IMG006, IMG007, IMG008, A11Y001, A11Y002, A11Y003, A11Y004, A11Y005,
    A11Y006, A11Y007, A11Y008, A11Y009, A11Y010, A11Y011, A11Y012, A11Y013,
];
