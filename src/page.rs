//! Reading one content document: what it declares and what it renders.
//!
//! Known limits, stated once here so the report can repeat them: images
//! brought in by CSS (`background-image`) are not seen, and neither is
//! anything a script builds at runtime. A fixed-layout comic that hides its
//! artwork in a stylesheet will look empty to this tool, which is itself worth
//! knowing -- a reading system's accessibility tree sees about as much.

use crate::model::{PageImage, ViewportSource};
use crate::util;
use crate::xml::{self, Node};

/// The facts a single content document contributes.
#[derive(Debug, Default)]
pub struct PageParse {
    pub viewport: Option<(u32, u32)>,
    pub viewport_source: ViewportSource,
    pub viewport_raw: Option<String>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub images: Vec<PageImage>,
    pub text_len: usize,
    pub parse_error: Option<String>,
}

/// Parse an XHTML or SVG content document.
///
/// `path` is the document's own path inside the container, used to resolve
/// the hrefs it carries.
pub fn parse(path: &str, media_type: &str, text: &str) -> PageParse {
    let mut out = PageParse::default();
    let is_svg_document = media_type.contains("svg");

    // Where we are: element stack (lowercased local names) plus a couple of
    // flags that are cheaper to keep than to recompute from the stack.
    let mut stack: Vec<String> = Vec::new();
    let mut in_title = false;
    let mut title_buf = String::new();
    let mut text_buf = String::new();
    let mut svg_depth = 0usize;
    // Index of the first image of the current SVG, so a <title> found later
    // in the same SVG can still describe it.
    let mut svg_image_start: Option<usize> = None;
    let mut svg_title_buf: Option<String> = None;
    let mut in_svg_title = false;

    let note = xml::scan(text, |node| match node {
        Node::Start(e) => {
            match e.name.as_str() {
                "html" | "svg" if out.lang.is_none() => {
                    if let Some(lang) = e.attr("xml:lang").or_else(|| e.attr("lang")) {
                        if !lang.trim().is_empty() {
                            out.lang = Some(lang.trim().to_string());
                        }
                    }
                }
                _ => {}
            }

            match e.name.as_str() {
                "title" => {
                    if svg_depth > 0 {
                        in_svg_title = svg_title_buf.is_none();
                        if in_svg_title {
                            svg_title_buf = Some(String::new());
                        }
                    } else if out.title.is_none() {
                        in_title = true;
                        title_buf.clear();
                    }
                }
                "meta" => {
                    let name = e.attr("name").unwrap_or("").to_ascii_lowercase();
                    if name == "viewport" {
                        let content = e.attr("content").unwrap_or("").to_string();
                        if out.viewport_raw.is_none() {
                            out.viewport_raw = Some(content.clone());
                            out.viewport_source = ViewportSource::MetaTag;
                            out.viewport = parse_viewport(&content);
                        }
                    }
                }
                "svg" => {
                    svg_depth += 1;
                    if svg_depth == 1 {
                        svg_image_start = Some(out.images.len());
                        svg_title_buf = None;
                    }
                    // An SVG spine item is itself the page, so its own
                    // dimensions are the viewport.
                    if is_svg_document && out.viewport.is_none() && stack.is_empty() {
                        let raw = format!(
                            "width={} height={} viewBox={}",
                            e.attr("width").unwrap_or("-"),
                            e.attr("height").unwrap_or("-"),
                            e.attr("viewbox").unwrap_or("-")
                        );
                        out.viewport_raw = Some(raw);
                        out.viewport_source = ViewportSource::SvgAttributes;
                        out.viewport =
                            svg_viewport(e.attr("width"), e.attr("height"), e.attr("viewbox"));
                    }
                }
                "img" => {
                    let src = e.attr("src").unwrap_or("").trim().to_string();
                    if !src.is_empty() {
                        out.images.push(image_ref(path, &src, "img", &e));
                    }
                }
                "image" => {
                    let href = e
                        .attr("xlink:href")
                        .or_else(|| e.attr("href"))
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    if !href.is_empty() {
                        out.images.push(image_ref(path, &href, "image", &e));
                    }
                }
                "object" => {
                    let data = e.attr("data").unwrap_or("").trim().to_string();
                    let is_image = e
                        .attr("type")
                        .map(|t| t.starts_with("image/"))
                        .unwrap_or(false);
                    if is_image && !data.is_empty() {
                        out.images.push(image_ref(path, &data, "object", &e));
                    }
                }
                _ => {}
            }

            if !e.empty {
                stack.push(e.name.clone());
            } else if e.name == "svg" {
                svg_depth = svg_depth.saturating_sub(1);
            }
        }
        Node::End(name) => {
            match name.as_str() {
                "title" => {
                    if in_svg_title {
                        in_svg_title = false;
                    } else if in_title {
                        in_title = false;
                        let t = xml::collapse(&title_buf);
                        out.title = Some(t);
                    }
                }
                "svg" => {
                    if svg_depth == 1 {
                        // Attribute the SVG's own <title> to the images it
                        // wraps, which is how a reading system announces a
                        // page drawn as one <image> inside an <svg>.
                        if let (Some(start), Some(t)) = (svg_image_start, svg_title_buf.take()) {
                            let t = xml::collapse(&t);
                            for img in out.images.iter_mut().skip(start) {
                                if img.alt.is_none() && !t.is_empty() {
                                    img.alt = Some(t.clone());
                                }
                            }
                        }
                        svg_image_start = None;
                    }
                    svg_depth = svg_depth.saturating_sub(1);
                }
                _ => {}
            }
            if let Some(pos) = stack.iter().rposition(|s| *s == name) {
                stack.truncate(pos);
            }
        }
        Node::Text(t) => {
            if in_svg_title {
                if let Some(buf) = svg_title_buf.as_mut() {
                    buf.push_str(&t);
                }
                return;
            }
            if in_title {
                title_buf.push_str(&t);
                return;
            }
            let inside_head_or_code = stack
                .iter()
                .any(|s| s == "script" || s == "style" || s == "head");
            if !inside_head_or_code {
                text_buf.push_str(&t);
                text_buf.push(' ');
            }
        }
    });

    out.parse_error = note;
    out.text_len = xml::collapse(&text_buf).chars().count();
    out
}

fn image_ref(base: &str, href: &str, element: &'static str, e: &xml::Element) -> PageImage {
    let remote = util::is_remote(href);
    let path = if remote {
        href.to_string()
    } else {
        util::resolve(base, href)
    };
    let alt = e
        .attr("alt")
        .map(|s| s.to_string())
        .or_else(|| e.attr("aria-label").map(|s| s.to_string()));
    PageImage {
        path,
        remote,
        element,
        alt,
        aria_hidden: e.attr("aria-hidden").map(|v| v == "true").unwrap_or(false),
        role: e.attr("role").map(|s| s.to_string()),
        pixels: None,
        bytes: None,
    }
}

/// Read `width` and `height` out of a viewport meta declaration.
///
/// Only pixel values count: `width=device-width` is a reflowable idiom and
/// leaves a fixed-layout page without dimensions, which is a finding, not a
/// value to invent.
pub fn parse_viewport(content: &str) -> Option<(u32, u32)> {
    let mut width = None;
    let mut height = None;
    for part in content.split([',', ';']) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_end_matches("px").trim();
        let parsed = value.parse::<f64>().ok().filter(|v| *v > 0.0);
        match key.as_str() {
            "width" => width = parsed,
            "height" => height = parsed,
            _ => {}
        }
    }
    match (width, height) {
        (Some(w), Some(h)) => Some((w.round() as u32, h.round() as u32)),
        _ => None,
    }
}

/// Viewport of an SVG spine item: explicit width/height in pixels, or the
/// size half of the viewBox when they are absent or given in percent.
pub fn svg_viewport(
    width: Option<&str>,
    height: Option<&str>,
    view_box: Option<&str>,
) -> Option<(u32, u32)> {
    let px = |v: Option<&str>| -> Option<f64> {
        let v = v?.trim().trim_end_matches("px").trim();
        v.parse::<f64>().ok().filter(|n| *n > 0.0)
    };
    if let (Some(w), Some(h)) = (px(width), px(height)) {
        return Some((w.round() as u32, h.round() as u32));
    }
    let vb = view_box?;
    let nums: Vec<f64> = vb
        .split([' ', ','])
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<f64>().ok())
        .collect();
    if nums.len() == 4 && nums[2] > 0.0 && nums[3] > 0.0 {
        return Some((nums[2].round() as u32, nums[3].round() as u32));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_meta_needs_pixels() {
        assert_eq!(
            parse_viewport("width=1200, height=1800"),
            Some((1200, 1800))
        );
        assert_eq!(
            parse_viewport("width=1200px;height=1800px"),
            Some((1200, 1800))
        );
        // The reflowable idiom carries no page size, and inventing one would
        // hide the finding this tool exists to make.
        assert_eq!(parse_viewport("width=device-width, initial-scale=1"), None);
        assert_eq!(parse_viewport("height=1800"), None);
    }

    #[test]
    fn svg_pages_fall_back_to_the_view_box() {
        assert_eq!(
            svg_viewport(Some("1200"), Some("1800"), None),
            Some((1200, 1800))
        );
        assert_eq!(
            svg_viewport(Some("100%"), Some("100%"), Some("0 0 1200 1800")),
            Some((1200, 1800))
        );
        assert_eq!(
            svg_viewport(None, None, Some("0,0,800,1280")),
            Some((800, 1280))
        );
        assert_eq!(svg_viewport(None, None, None), None);
    }

    #[test]
    fn a_page_is_read_down_to_its_image_and_its_text() {
        let doc = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="ja" lang="ja">
<head><title>Page 3</title><meta name="viewport" content="width=1200, height=1800"/>
<style>body { margin: 0 }</style></head>
<body><div><img src="../images/p003.png" alt="Aoi &amp; Rin under the awning."/></div></body>
</html>"#;
        let parsed = parse("OEBPS/text/p003.xhtml", "application/xhtml+xml", doc);
        assert_eq!(parsed.viewport, Some((1200, 1800)));
        assert_eq!(parsed.viewport_source, ViewportSource::MetaTag);
        assert_eq!(parsed.title.as_deref(), Some("Page 3"));
        assert_eq!(parsed.lang.as_deref(), Some("ja"));
        assert_eq!(parsed.images.len(), 1);
        assert_eq!(parsed.images[0].path, "OEBPS/images/p003.png");
        assert_eq!(
            parsed.images[0].alt.as_deref(),
            Some("Aoi & Rin under the awning.")
        );
        // A style rule is not page text, so the page still counts as one image.
        assert_eq!(parsed.text_len, 0);
    }

    #[test]
    fn an_svg_wrapped_page_takes_its_description_from_the_svg_title() {
        let doc = r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:xlink="http://www.w3.org/1999/xlink" lang="en">
<head><title>Page 1</title><meta name="viewport" content="width=1200, height=1800"/></head>
<body><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 1800" role="img">
<title>The shop shutters come down as Rin arrives.</title>
<image width="1200" height="1800" xlink:href="../images/p001.png"/>
</svg></body></html>"#;
        let parsed = parse("OEBPS/text/p001.xhtml", "application/xhtml+xml", doc);
        assert_eq!(parsed.images.len(), 1);
        assert_eq!(parsed.images[0].element, "image");
        assert_eq!(
            parsed.images[0].alt.as_deref(),
            Some("The shop shutters come down as Rin arrives.")
        );
        // The SVG title describes the artwork; it is not page text.
        assert_eq!(parsed.text_len, 0);
    }

    #[test]
    fn an_svg_spine_item_is_its_own_viewport() {
        let doc = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="1800">
<image width="1200" height="1800" href="p001.png"/></svg>"#;
        let parsed = parse("OEBPS/p001.svg", "image/svg+xml", doc);
        assert_eq!(parsed.viewport, Some((1200, 1800)));
        assert_eq!(parsed.viewport_source, ViewportSource::SvgAttributes);
        assert_eq!(parsed.images[0].path, "OEBPS/p001.png");
    }

    #[test]
    fn a_malformed_document_reports_what_it_managed_to_read() {
        let doc = "<html><head><title>Page 1</title><meta name=\"viewport\" content=\"width=10, height=20\">\
                   <body><img src=\"a.png\" alt=\"one\">";
        let parsed = parse("OEBPS/p.xhtml", "application/xhtml+xml", doc);
        assert_eq!(parsed.viewport, Some((10, 20)));
        assert_eq!(parsed.images.len(), 1);
    }
}
