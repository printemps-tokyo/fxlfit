//! Black-box tests of the binary.
//!
//! Every EPUB in the suite is built here, in code, as a real ZIP container:
//! no binary fixture is checked in, and each test says in its own body exactly
//! what is wrong with the book it builds.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

fn bin() -> PathBuf {
    let mut p = std::env::current_exe().expect("test exe path");
    p.pop(); // the deps/ directory
    if p.ends_with("deps") {
        p.pop();
    }
    p.join(format!("fxlfit{}", std::env::consts::EXE_SUFFIX))
}

fn run(args: &[&str]) -> Output {
    Command::new(bin()).args(args).output().expect("run fxlfit")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

/// Which check ids the report mentions, in the order they were printed.
fn ids(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let severity = parts.next()?;
            if !matches!(severity, "error" | "warn" | "info") {
                return None;
            }
            let id = parts.next()?;
            let looks_like_id = id
                .chars()
                .next()
                .map(|c| c.is_ascii_uppercase())
                .unwrap_or(false);
            looks_like_id.then(|| id.to_string())
        })
        .collect()
}

fn reports(text: &str, id: &str) -> bool {
    ids(text).iter().any(|found| found == id)
}

// ---------------------------------------------------------------------------
// Book builder
// ---------------------------------------------------------------------------

/// A PNG header, which is all the tool ever reads: the pixel size lives in
/// IHDR and no decoder is involved, so the image data is not needed to test
/// what the report says about dimensions.
fn png(width: u32, height: u32) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for byte in bytes {
            crc ^= *byte as u32;
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }
    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
        out
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend_from_slice(&chunk(b"IHDR", &ihdr));
    out.extend_from_slice(&chunk(b"IEND", &[]));
    out
}

#[derive(Clone)]
struct PageSpec {
    viewport: Option<(u32, u32)>,
    /// Overrides `viewport` with a literal content attribute, for the cases
    /// where the declaration exists but carries no pixel size.
    viewport_literal: Option<String>,
    image: (u32, u32),
    alt: Option<String>,
    title: Option<String>,
    lang: Option<String>,
    aria_hidden: bool,
    spread: Option<String>,
}

impl PageSpec {
    fn good(n: usize) -> PageSpec {
        PageSpec {
            viewport: Some((1200, 1800)),
            viewport_literal: None,
            image: (1200, 1800),
            alt: Some(format!(
                "Page {n}: Aoi turns the corner and finds the shop already closed."
            )),
            title: Some(format!("Page {n}")),
            lang: Some("ja".to_string()),
            aria_hidden: false,
            spread: Some(if n % 2 == 1 {
                "right".into()
            } else {
                "left".into()
            }),
        }
    }
}

/// A fixed-layout comic that passes every check, and the knobs each test
/// turns to break exactly one thing.
#[derive(Clone)]
struct Comic {
    pages: Vec<PageSpec>,
    declare_layout: bool,
    declare_spread: bool,
    page_progression: Option<String>,
    language: String,
    a11y_metadata: String,
    toc_entries: usize,
    page_list: bool,
    extra_files: Vec<(String, Vec<u8>)>,
}

const GOOD_A11Y: &str = r#"
  <meta property="schema:accessMode">visual</meta>
  <meta property="schema:accessMode">textual</meta>
  <meta property="schema:accessModeSufficient">textual</meta>
  <meta property="schema:accessibilityFeature">alternativeText</meta>
  <meta property="schema:accessibilityHazard">none</meta>
  <meta property="schema:accessibilitySummary">Every page image is described. The text is part of the artwork and does not reflow.</meta>
  <meta property="dcterms:conformsTo">EPUB Accessibility 1.1 - WCAG 2.2 Level AA</meta>
  <meta property="a11y:certifiedBy">printemps.tokyo</meta>
"#;

impl Default for Comic {
    fn default() -> Comic {
        Comic {
            pages: (1..=4).map(PageSpec::good).collect(),
            declare_layout: true,
            declare_spread: true,
            page_progression: Some("rtl".to_string()),
            language: "ja".to_string(),
            a11y_metadata: GOOD_A11Y.to_string(),
            toc_entries: 2,
            page_list: true,
            extra_files: Vec::new(),
        }
    }
}

impl Comic {
    fn build(&self, dir: &Path, name: &str) -> PathBuf {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        files.push((
            "META-INF/container.xml".to_string(),
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#
                .to_vec(),
        ));

        let mut manifest = String::new();
        let mut spine = String::new();

        for (i, page) in self.pages.iter().enumerate() {
            let n = i + 1;
            let image = format!("p{n:03}.png");
            files.push((
                format!("OEBPS/images/{image}"),
                png(page.image.0, page.image.1),
            ));

            let viewport = match (page.viewport_literal.as_deref(), page.viewport) {
                (Some(literal), _) => format!("<meta name=\"viewport\" content=\"{literal}\"/>"),
                (None, Some((w, h))) => {
                    format!("<meta name=\"viewport\" content=\"width={w}, height={h}\"/>")
                }
                (None, None) => String::new(),
            };
            let title = page
                .title
                .as_deref()
                .map(|t| format!("<title>{t}</title>"))
                .unwrap_or_default();
            let lang = page
                .lang
                .as_deref()
                .map(|l| format!(" xml:lang=\"{l}\" lang=\"{l}\""))
                .unwrap_or_default();
            let alt = page
                .alt
                .as_deref()
                .map(|a| format!(" alt=\"{a}\""))
                .unwrap_or_default();
            let hidden = if page.aria_hidden {
                " aria-hidden=\"true\""
            } else {
                ""
            };

            files.push((
                format!("OEBPS/text/p{n:03}.xhtml"),
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"{lang}>
<head>{title}{viewport}</head>
<body><div><img src="../images/{image}"{alt}{hidden}/></div></body>
</html>"#
                )
                .into_bytes(),
            ));

            let cover = if n == 1 {
                " properties=\"cover-image\""
            } else {
                ""
            };
            manifest.push_str(&format!(
                "<item id=\"p{n}\" href=\"text/p{n:03}.xhtml\" media-type=\"application/xhtml+xml\"/>\
                 <item id=\"i{n}\" href=\"images/{image}\" media-type=\"image/png\"{cover}/>"
            ));
            let props = page
                .spread
                .as_deref()
                .map(|s| format!(" properties=\"page-spread-{s}\""))
                .unwrap_or_default();
            spine.push_str(&format!("<itemref idref=\"p{n}\"{props}/>"));
        }

        manifest.push_str(
            "<item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>",
        );

        let toc: String = (0..self.toc_entries)
            .map(|i| {
                format!(
                    "<li><a href=\"text/p{:03}.xhtml\">Episode {}</a></li>",
                    i + 1,
                    i + 1
                )
            })
            .collect();
        let page_list = if self.page_list {
            "<nav epub:type=\"page-list\" hidden=\"\"><ol><li><a href=\"text/p001.xhtml\">1</a></li></ol></nav>"
        } else {
            ""
        };
        files.push((
            "OEBPS/nav.xhtml".to_string(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="ja" xml:lang="ja">
<head><title>Contents</title></head>
<body><nav epub:type="toc"><ol>{toc}</ol></nav>{page_list}</body></html>"#
            )
            .into_bytes(),
        ));

        let layout = if self.declare_layout {
            "<meta property=\"rendition:layout\">pre-paginated</meta>"
        } else {
            ""
        };
        let spread_meta = if self.declare_spread {
            "<meta property=\"rendition:spread\">both</meta>"
        } else {
            ""
        };
        let ppd = self
            .page_progression
            .as_deref()
            .map(|d| format!(" page-progression-direction=\"{d}\""))
            .unwrap_or_default();

        files.push((
            "OEBPS/content.opf".to_string(),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id" prefix="a11y: http://www.idpf.org/epub/vocab/package/a11y/#">
 <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
  <dc:title>Test Comic</dc:title>
  <dc:language>{language}</dc:language>
  <dc:identifier id="pub-id">urn:uuid:test</dc:identifier>
  <meta property="dcterms:modified">2026-08-24T00:00:00Z</meta>
  {layout}{spread_meta}{a11y}
 </metadata>
 <manifest>{manifest}</manifest>
 <spine{ppd}>{spine}</spine>
</package>"#,
                language = self.language,
                a11y = self.a11y_metadata,
            )
            .into_bytes(),
        ));

        files.extend(self.extra_files.iter().cloned());

        let path = dir.join(name);
        let file = File::create(&path).expect("create epub");
        let mut zip = ZipWriter::new(file);
        zip.start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .expect("mimetype");
        zip.write_all(b"application/epub+zip")
            .expect("mimetype body");
        for (name, data) in files {
            zip.start_file(
                &name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("entry");
            zip.write_all(&data).expect("entry body");
        }
        zip.finish().expect("finish zip");
        path
    }
}

fn check(comic: &Comic) -> (String, i32, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");
    let out = run(&[path.to_str().unwrap(), "--no-color"]);
    (stdout(&out), code(&out), dir)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn a_well_formed_comic_has_nothing_to_report() {
    let (report, code, _dir) = check(&Comic::default());
    assert!(
        report.contains("verdict: READY"),
        "expected a clean verdict, got:\n{report}"
    );
    assert_eq!(
        ids(&report),
        Vec::<String>::new(),
        "unexpected findings:\n{report}"
    );
    assert_eq!(code, 0);
}

#[test]
fn a_page_without_a_viewport_is_an_error() {
    let mut comic = Comic::default();
    comic.pages[1].viewport = None;
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "FXL003"), "{report}");
    assert!(report.contains("p002.xhtml (page 2)"), "{report}");
    assert_eq!(code, 1);
}

#[test]
fn device_width_is_not_a_fixed_layout_viewport() {
    let mut comic = Comic::default();
    comic.pages[0].viewport_literal = Some("width=device-width, initial-scale=1".to_string());
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "FXL011"), "{report}");
    assert!(report.contains("width=device-width"), "{report}");
    // The page declared something, so it is not also reported as missing.
    assert!(!reports(&report, "FXL003"), "{report}");
    assert_eq!(code, 1);
}

#[test]
fn a_missing_alt_attribute_is_an_error() {
    let mut comic = Comic::default();
    comic.pages[0].alt = None;
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y001"), "{report}");
    assert_eq!(code, 1);
}

#[test]
fn an_image_only_page_marked_decorative_is_an_error() {
    let mut comic = Comic::default();
    comic.pages[2].alt = Some(String::new());
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y013"), "{report}");
    assert!(report.contains("alt=\"\""), "{report}");
    assert_eq!(code, 1);

    let mut hidden = Comic::default();
    hidden.pages[2].aria_hidden = true;
    let (report, _, _dir) = check(&hidden);
    assert!(reports(&report, "A11Y013"), "{report}");
}

#[test]
fn filename_alt_text_is_reported_as_a_placeholder() {
    let mut comic = Comic::default();
    comic.pages[0].alt = Some("p001.png".to_string());
    comic.pages[1].alt = Some("page 2".to_string());
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y002"), "{report}");
    assert!(report.contains("it is the file name"), "{report}");
}

#[test]
fn the_same_alt_on_every_page_is_reported() {
    let mut comic = Comic::default();
    for page in comic.pages.iter_mut() {
        page.alt = Some("A page of the comic showing the characters talking.".to_string());
    }
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y003"), "{report}");
}

#[test]
fn metadata_that_overstates_the_book_is_reported() {
    let mut comic = Comic::default();
    comic.pages[0].alt = None;
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y008"), "{report}");
    assert!(
        report.contains("accessibilityFeature says alternativeText"),
        "{report}"
    );
}

#[test]
fn a_conformance_string_that_does_not_parse_is_reported() {
    let mut comic = Comic::default();
    comic.a11y_metadata = comic.a11y_metadata.replace(
        "EPUB Accessibility 1.1 - WCAG 2.2 Level AA",
        "WCAG 2.2 AA compliant",
    );
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y006"), "{report}");
    assert!(report.contains("not in the required form"), "{report}");
}

#[test]
fn a_lower_level_claim_is_measured_against_the_target() {
    let mut comic = Comic::default();
    comic.a11y_metadata = comic
        .a11y_metadata
        .replace("WCAG 2.2 Level AA", "WCAG 2.0 Level A");
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y006"), "{report}");

    // Asking for the level the book actually claims makes the finding go away.
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");
    let out = run(&[
        path.to_str().unwrap(),
        "--no-color",
        "--level",
        "a",
        "--only",
        "A11Y006",
    ]);
    assert_eq!(code(&out), 0, "{}", stdout(&out));
}

#[test]
fn required_discovery_metadata_is_required() {
    let comic = Comic {
        a11y_metadata: String::new(),
        ..Comic::default()
    };
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "A11Y004"), "{report}");
    assert!(reports(&report, "A11Y005"), "{report}");
    assert!(reports(&report, "A11Y006"), "{report}");
    assert_eq!(code, 1);
}

#[test]
fn an_undersized_page_image_is_reported() {
    let mut comic = Comic::default();
    comic.pages[1].image = (600, 900);
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "IMG003"), "{report}");
    assert!(report.contains("0.50x"), "{report}");
}

#[test]
fn an_image_that_does_not_fit_its_viewport_is_reported() {
    let mut comic = Comic::default();
    comic.pages[1].image = (1200, 1200);
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "FXL005"), "{report}");
}

#[test]
fn a_japanese_book_without_page_progression_is_an_error() {
    let comic = Comic {
        page_progression: None,
        ..Comic::default()
    };
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "FXL008"), "{report}");
    assert!(report.contains("right-to-left"), "{report}");
    assert_eq!(code, 1);

    // The same book in English is a warning, not an error.
    let mut english = comic.clone();
    english.language = "en".to_string();
    let (report, code, _dir) = check(&english);
    assert!(reports(&report, "FXL008"), "{report}");
    assert_eq!(code, 0, "{report}");
}

#[test]
fn pages_that_are_fixed_layout_in_all_but_the_declaration_are_still_checked() {
    let mut comic = Comic {
        declare_layout: false,
        ..Comic::default()
    };
    comic.pages[0].viewport = None;
    let (report, code, _dir) = check(&comic);
    assert!(reports(&report, "FXL001"), "{report}");
    assert!(
        report.contains("run against them anyway"),
        "the report must say the layout checks continued:\n{report}"
    );
    assert!(reports(&report, "FXL003"), "{report}");
    assert_eq!(code, 1);
}

#[test]
fn spread_sides_have_to_alternate() {
    let mut comic = Comic::default();
    comic.pages[1].spread = Some("right".to_string());
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "FXL007"), "{report}");
    assert!(report.contains("do not alternate"), "{report}");
}

#[test]
fn an_unreferenced_resource_is_reported() {
    let mut comic = Comic::default();
    comic
        .extra_files
        .push(("OEBPS/images/leftover.png".to_string(), png(800, 800)));
    let (report, _code, _dir) = check(&comic);
    assert!(reports(&report, "PKG006"), "{report}");
    assert!(report.contains("leftover.png"), "{report}");
}

#[test]
fn the_per_image_budget_is_the_callers_to_set() {
    let comic = Comic::default();
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");

    let out = run(&[
        path.to_str().unwrap(),
        "--no-color",
        "--max-image-bytes",
        "10",
    ]);
    assert!(reports(&stdout(&out), "IMG002"), "{}", stdout(&out));

    let out = run(&[
        path.to_str().unwrap(),
        "--no-color",
        "--max-image-bytes",
        "1MiB",
    ]);
    assert!(!reports(&stdout(&out), "IMG002"), "{}", stdout(&out));
}

#[test]
fn only_and_skip_select_checks() {
    let mut comic = Comic::default();
    comic.pages[0].alt = None;
    comic.pages[0].viewport = None;
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");

    let out = run(&[path.to_str().unwrap(), "--no-color", "--only", "access"]);
    let report = stdout(&out);
    assert!(reports(&report, "A11Y001"), "{report}");
    assert!(!reports(&report, "FXL003"), "{report}");

    let out = run(&[path.to_str().unwrap(), "--no-color", "--skip", "A11Y,FXL"]);
    let report = stdout(&out);
    assert!(!reports(&report, "A11Y001"), "{report}");
    assert!(!reports(&report, "FXL003"), "{report}");
}

#[test]
fn fail_on_controls_the_exit_code() {
    let comic = Comic {
        toc_entries: 1, // a warning, nothing worse
        ..Comic::default()
    };
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");

    assert_eq!(code(&run(&[path.to_str().unwrap(), "--no-color"])), 0);
    assert_eq!(
        code(&run(&[
            path.to_str().unwrap(),
            "--no-color",
            "--fail-on",
            "warn"
        ])),
        1
    );
    assert_eq!(
        code(&run(&[
            path.to_str().unwrap(),
            "--no-color",
            "--fail-on",
            "never"
        ])),
        0
    );
}

#[test]
fn the_json_report_carries_the_same_verdict() {
    let mut comic = Comic::default();
    comic.pages[0].alt = None;
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");

    let out = run(&[path.to_str().unwrap(), "--json"]);
    let json = stdout(&out);
    assert!(json.starts_with("{\"tool\":\"fxlfit\""), "{json}");
    assert!(json.contains("\"verdict\":\"NOT READY\""), "{json}");
    assert!(json.contains("\"id\":\"A11Y001\""), "{json}");
    assert!(json.contains("\"viewport\":[1200,1800]"), "{json}");
    assert!(json.ends_with("}\n"), "{json}");
    assert_eq!(code(&out), 1);
}

#[test]
fn the_page_table_lists_the_spine_in_order() {
    let comic = Comic::default();
    let dir = TempDir::new().expect("tempdir");
    let path = comic.build(dir.path(), "book.epub");
    let report = stdout(&run(&[path.to_str().unwrap(), "--no-color", "--pages"]));

    let table: Vec<&str> = report
        .lines()
        .filter(|l| l.contains("p00") && l.contains("fixed"))
        .collect();
    assert_eq!(table.len(), 4, "{report}");
    assert!(table[0].contains("1200x1800"), "{report}");
}

#[test]
fn the_catalogue_is_printable_without_a_book() {
    let out = run(&["--list-checks", "--no-color"]);
    let text = stdout(&out);
    assert!(text.contains("FXL003"), "{text}");
    assert!(text.contains("A11Y004"), "{text}");
    assert_eq!(code(&out), 0);
}

#[test]
fn usage_errors_exit_two() {
    assert_eq!(code(&run(&["--no-color"])), 2);

    let dir = TempDir::new().expect("tempdir");
    let comic = Comic::default();
    let path = comic.build(dir.path(), "book.epub");
    assert_eq!(
        code(&run(&[path.to_str().unwrap(), "--only", "NOSUCHCHECK"])),
        2
    );

    let not_an_epub = dir.path().join("notes.txt");
    std::fs::write(&not_an_epub, b"this is not a zip").expect("write");
    assert_eq!(code(&run(&[not_an_epub.to_str().unwrap()])), 2);
}
