//! The command line.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Parser;

use fxlfit::checks::{self, Options};
use fxlfit::model::{Category, Severity};
use fxlfit::report::{self, Style};
use fxlfit::{epub, util};

/// Preflight a fixed-layout EPUB: layout and viewport sanity, image budgets,
/// and EPUB Accessibility 1.1 discovery metadata.
#[derive(Parser, Debug)]
#[command(
    name = "fxlfit",
    version,
    about = "Preflight fixed-layout EPUB comics and picture books",
    long_about = "Preflight a fixed-layout EPUB before you upload it.\n\n\
fxlfit reads the package document and every page, then reports what a comic or \
picture book gets wrong in practice: a book that says it is fixed-layout but \
is not, pages with no viewport, artwork that does not match the canvas it is \
drawn into, images nothing references, and the accessibility metadata an EU \
storefront has had to display since the European Accessibility Act took effect \
on 2025-06-28.\n\n\
It is a preflight, not a validator. Run epubcheck for conformance."
)]
struct Cli {
    /// The .epub file to inspect. Optional only with --list-checks.
    epub: Option<PathBuf>,

    /// Print the report as JSON.
    #[arg(long)]
    json: bool,

    /// Print a per-page table before the findings.
    #[arg(long)]
    pages: bool,

    /// Print the check catalogue and exit.
    #[arg(long)]
    list_checks: bool,

    /// Lowest severity that makes the run fail: error, warn, info or never.
    #[arg(long, default_value = "error", value_name = "SEVERITY")]
    fail_on: String,

    /// Budget for a single image, e.g. 5MiB, 800KB or a byte count.
    #[arg(long, default_value = "5MiB", value_name = "SIZE")]
    max_image_bytes: String,

    /// Budget for the whole publication, uncompressed.
    #[arg(long, default_value = "unlimited", value_name = "SIZE")]
    max_total_bytes: String,

    /// Report a page image as upscaled below this image-to-viewport ratio.
    #[arg(long, default_value_t = 1.0, value_name = "RATIO")]
    min_scale: f64,

    /// Allowed difference between an image's aspect ratio and its viewport's.
    #[arg(long, default_value_t = 2.0, value_name = "PERCENT")]
    aspect_tolerance: f64,

    /// WCAG version expected in the conformance string.
    #[arg(long, default_value = "2.2", value_name = "VERSION")]
    wcag: String,

    /// WCAG level expected in the conformance string: A, AA or AAA.
    #[arg(long, default_value = "AA", value_name = "LEVEL")]
    level: String,

    /// Run only these checks: ids, family prefixes or categories, comma separated.
    #[arg(long, value_name = "LIST")]
    only: Option<String>,

    /// Never run these checks: ids, family prefixes or categories, comma separated.
    #[arg(long, value_name = "LIST")]
    skip: Option<String>,

    /// Do not colorize the output.
    #[arg(long)]
    no_color: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let style = Style::new(cli.no_color);

    if cli.list_checks {
        print!("{}", report::render_catalog(&style));
        return ExitCode::SUCCESS;
    }

    match run(&cli, &style) {
        Ok(failed) => {
            if failed {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("fxlfit: {err:#}");
            ExitCode::from(2)
        }
    }
}

/// Returns true when the findings reached the `--fail-on` threshold.
fn run(cli: &Cli, style: &Style) -> Result<bool> {
    let opts = options(cli)?;
    let fail_on = match cli.fail_on.to_ascii_lowercase().as_str() {
        "never" | "none" => None,
        other => match Severity::parse(other) {
            Some(s) => Some(s),
            None => bail!("--fail-on takes error, warn, info or never (got {other})"),
        },
    };

    let Some(path) = cli.epub.as_ref() else {
        bail!("no EPUB given; pass a .epub file, or --list-checks to see what would be checked");
    };
    let book = epub::read(path)?;
    let findings = checks::run(&book, &opts);

    if cli.json {
        print!("{}", report::render_json(&book, &findings, &opts));
    } else {
        if cli.pages {
            print!("{}", report::render_pages(&book, style));
        }
        print!("{}", report::render_text(&book, &findings, &opts, style));
    }

    Ok(match fail_on {
        Some(threshold) => findings.iter().any(|f| f.severity >= threshold),
        None => false,
    })
}

fn options(cli: &Cli) -> Result<Options> {
    let split = |value: &Option<String>| -> Vec<String> {
        value
            .as_deref()
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    let only = split(&cli.only);
    let skip = split(&cli.skip);
    for pattern in only.iter().chain(skip.iter()) {
        let known = checks::catalog::ALL.iter().any(|c| {
            c.id.eq_ignore_ascii_case(pattern)
                || c.id
                    .to_ascii_uppercase()
                    .starts_with(&pattern.to_ascii_uppercase())
        }) || Category::parse(pattern).is_some();
        if !known {
            bail!("unknown check selector \"{pattern}\"; see --list-checks");
        }
    }

    if cli.min_scale <= 0.0 {
        bail!("--min-scale must be greater than 0");
    }
    if cli.aspect_tolerance < 0.0 {
        bail!("--aspect-tolerance cannot be negative");
    }
    let level = cli.level.to_ascii_uppercase();
    if !matches!(level.as_str(), "A" | "AA" | "AAA") {
        bail!("--level takes A, AA or AAA (got {})", cli.level);
    }

    Ok(Options {
        max_image_bytes: util::parse_size(&cli.max_image_bytes).map_err(|e| anyhow::anyhow!(e))?,
        max_total_bytes: util::parse_size(&cli.max_total_bytes).map_err(|e| anyhow::anyhow!(e))?,
        min_scale: cli.min_scale,
        aspect_tolerance: cli.aspect_tolerance,
        wcag: cli.wcag.clone(),
        level,
        only,
        skip,
    })
}
