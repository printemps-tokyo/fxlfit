//! Images and the size budget.
//!
//! A fixed-layout comic is almost entirely image bytes, and every one of them
//! is downloaded by every reader. These checks look for the two ways that
//! goes wrong: pages that are too small for the viewport they are drawn into
//! (blurry on delivery) and pages that are far larger than anything will ever
//! display (paid for, never seen).

use std::collections::BTreeMap;

use super::catalog::*;
use super::{page_label, Options};
use crate::model::{Book, Finding, Severity};
use crate::util;

/// EPUB 3.3 core media types for images. Anything else needs a manifest
/// fallback to be safe, and support outside the core set is uneven.
const CORE_IMAGE_TYPES: [&str; 5] = [
    "image/gif",
    "image/jpeg",
    "image/png",
    "image/svg+xml",
    "image/webp",
];

pub fn check(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    missing_and_oversized(book, opts, out);
    scaling(book, opts, out);
    formats(book, out);
    budget(book, opts, out);
    resolution_consistency(book, out);
}

fn missing_and_oversized(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    let mut missing: Vec<String> = Vec::new();
    let mut oversized: Vec<(String, u64)> = Vec::new();
    let mut seen: BTreeMap<String, bool> = BTreeMap::new();

    for page in &book.pages {
        for img in &page.images {
            if img.remote || seen.contains_key(&img.path) {
                continue;
            }
            seen.insert(img.path.clone(), true);
            match book.resources.get(&img.path) {
                None => missing.push(format!("{} referenced by {}", img.path, page.path)),
                Some(res) if res.bytes > opts.max_image_bytes => {
                    oversized.push((img.path.clone(), res.bytes))
                }
                Some(_) => {}
            }
        }
    }

    if !missing.is_empty() {
        out.push(
            Finding::new(
                &IMG001,
                format!(
                    "{} referenced image(s) are not in the container",
                    missing.len()
                ),
                util::summarize_list(&missing, 8),
                "add the files, or fix the src attributes; every one of these renders as a \
                 broken page",
            )
            .at(missing),
        );
    }

    if !oversized.is_empty() {
        oversized.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        let detail: Vec<String> = oversized
            .iter()
            .map(|(p, b)| format!("{p} ({})", util::human_bytes(*b)))
            .collect();
        out.push(
            Finding::new(
                &IMG002,
                format!(
                    "{} image(s) over the {} per-image budget",
                    oversized.len(),
                    util::human_bytes(opts.max_image_bytes)
                ),
                util::summarize_list(&detail, 8),
                "re-encode them, or raise --max-image-bytes if your store allows it; store \
                 limits differ and change, so this budget is yours to set",
            )
            .at(detail),
        );
    }
}

fn scaling(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    let mut upscaled: Vec<String> = Vec::new();
    let mut oversampled: Vec<String> = Vec::new();

    for page in book.fxl_pages() {
        let (Some((vw, vh)), true) = (page.viewport, page.is_image_only()) else {
            continue;
        };
        let Some((iw, ih)) = page.images[0].pixels else {
            continue;
        };
        if vw == 0 || vh == 0 {
            continue;
        }
        let scale = (iw as f64 / vw as f64).min(ih as f64 / vh as f64);
        if scale < opts.min_scale {
            upscaled.push(format!(
                "{}: image {iw}x{ih} into viewport {vw}x{vh} ({scale:.2}x)",
                page_label(page)
            ));
        } else if scale > 3.0 {
            oversampled.push(format!(
                "{}: image {iw}x{ih} into viewport {vw}x{vh} ({scale:.1}x)",
                page_label(page)
            ));
        }
    }

    if !upscaled.is_empty() {
        out.push(
            Finding::new(
                &IMG003,
                format!(
                    "{} page image(s) are smaller than their viewport",
                    upscaled.len()
                ),
                util::summarize_list(&upscaled, 8),
                format!(
                    "export the artwork at least at viewport size (currently flagged below \
                     {:.2}x); a reading system upscales what it is given, and line art shows it \
                     first",
                    opts.min_scale
                ),
            )
            .at(upscaled),
        );
    }

    if !oversampled.is_empty() {
        out.push(
            Finding::new(
                &IMG004,
                format!(
                    "{} page image(s) are more than 3x their viewport",
                    oversampled.len()
                ),
                util::summarize_list(&oversampled, 8),
                "downsample to about 2x the viewport; beyond that the reader pays for pixels no \
                 device shows, unless you are shipping a print-quality edition on purpose",
            )
            .at(oversampled),
        );
    }
}

fn formats(book: &Book, out: &mut Vec<Finding>) {
    let mut foreign: Vec<String> = Vec::new();
    let mut mislabelled: Vec<String> = Vec::new();

    for item in &book.package.items {
        if item.remote || !item.media_type.starts_with("image/") {
            continue;
        }
        if !CORE_IMAGE_TYPES.contains(&item.media_type.as_str()) && item.fallback.is_none() {
            foreign.push(format!("{} ({})", item.path, item.media_type));
        }
    }

    // What the manifest says versus what the bytes are. A JPEG shipped as
    // image/png is read by some engines and rejected by others.
    let mut sniffed: BTreeMap<&str, &str> = BTreeMap::new();
    for page in &book.pages {
        for img in &page.images {
            if img.remote {
                continue;
            }
            let Some(item) = book.package.item_by_path(&img.path) else {
                continue;
            };
            let expected = match util::extension(&img.path).as_str() {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "svg" => "image/svg+xml",
                _ => continue,
            };
            if !item.media_type.is_empty() && item.media_type != expected {
                sniffed.insert(item.path.as_str(), expected);
                mislabelled.push(format!(
                    "{}: manifest says {}, the extension says {expected}",
                    item.path, item.media_type
                ));
            }
        }
    }

    if !foreign.is_empty() {
        out.push(
            Finding::new(
                &IMG006,
                format!(
                    "{} image(s) outside the EPUB 3 core media types",
                    foreign.len()
                ),
                util::summarize_list(&foreign, 8),
                "convert them to PNG, JPEG, GIF, SVG or WebP, or declare a manifest fallback; \
                 anything else is a resource a reading system may refuse",
            )
            .at(foreign),
        );
    }

    if !mislabelled.is_empty() {
        mislabelled.dedup();
        out.push(
            Finding::new(
                &IMG007,
                format!(
                    "{} image(s) carry a media-type that does not match",
                    mislabelled.len()
                ),
                util::summarize_list(&mislabelled, 8),
                "correct the media-type in the manifest",
            )
            .at(mislabelled),
        );
    }
}

fn budget(book: &Book, opts: &Options, out: &mut Vec<Finding>) {
    if book.total_bytes <= opts.max_total_bytes {
        return;
    }
    out.push(
        Finding::new(
            &IMG005,
            format!(
                "the publication is {} uncompressed, over the {} budget",
                util::human_bytes(book.total_bytes),
                util::human_bytes(opts.max_total_bytes)
            ),
            format!(
                "the .epub file itself is {} on disk",
                util::human_bytes(book.file_bytes)
            ),
            "downsample the page images, or raise --max-total-bytes; delivery limits are set by \
             the store, not by the format",
        )
        .at([book.file.clone()])
        .with_severity(Severity::Warn),
    );
}

fn resolution_consistency(book: &Book, out: &mut Vec<Finding>) {
    let mut sizes: BTreeMap<(u32, u32), Vec<String>> = BTreeMap::new();
    for page in book.fxl_pages() {
        if !page.is_image_only() {
            continue;
        }
        if let Some(px) = page.images[0].pixels {
            sizes.entry(px).or_default().push(page_label(page));
        }
    }
    if sizes.len() < 2 {
        return;
    }

    let dominant = sizes
        .iter()
        .max_by_key(|(_, pages)| pages.len())
        .map(|(size, _)| *size)
        .unwrap_or((0, 0));

    let mut odd: Vec<String> = Vec::new();
    for (size, pages) in &sizes {
        if *size == dominant {
            continue;
        }
        // A double-width spread page is normal; so is a cover that differs
        // slightly. Only sizes that differ in a way a reader would notice are
        // reported.
        if size.1 == dominant.1 && size.0 == dominant.0 * 2 {
            continue;
        }
        let height_drift = (size.1 as f64 - dominant.1 as f64).abs() / dominant.1.max(1) as f64;
        if height_drift < 0.02 {
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
        out.push(
            Finding::new(
                &IMG008,
                format!(
                    "page images come in {} different sizes (most are {}x{})",
                    sizes.len(),
                    dominant.0,
                    dominant.1
                ),
                util::summarize_list(&odd, 8),
                "export every page from the same canvas; mixed resolutions show up as pages \
                 that suddenly look softer or sharper than their neighbours",
            )
            .at(odd),
        );
    }
}
