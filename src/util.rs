//! Path, URL and size helpers shared by the reader and the checks.

/// Percent-decode a URL path segment sequence. EPUB hrefs are URLs, so a file
/// called `page 01.xhtml` is authored as `page%2001.xhtml` and has to be
/// matched back to the ZIP entry.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True for an href that points outside the container.
pub fn is_remote(href: &str) -> bool {
    let lower = href.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ftp://")
        || lower.starts_with("//")
        || lower.starts_with("data:")
        || lower.starts_with("mailto:")
}

/// Resolve an href against the directory of the document that carries it, and
/// normalize the result to a container path with no `.` or `..` left in it.
///
/// `base` is the path of the referring document inside the container, e.g.
/// `OEBPS/text/p001.xhtml`.
pub fn resolve(base: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or("").trim();
    let decoded = percent_decode(href);

    let mut parts: Vec<String> = Vec::new();
    if !decoded.starts_with('/') {
        let base_dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        if !base_dir.is_empty() {
            parts.extend(base_dir.split('/').map(|s| s.to_string()));
        }
    }

    for seg in decoded.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_string()),
        }
    }
    parts.join("/")
}

/// Last path segment, for messages and heuristics over filenames.
pub fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn extension(path: &str) -> String {
    basename(path)
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Parse a byte budget: plain bytes, or a `KB`/`KiB`/`MB`/`MiB`/`GB`/`GiB`
/// suffix. Decimal and binary units are both accepted and mean what they say.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim().to_ascii_lowercase().replace(' ', "");
    if t.is_empty() {
        return Err("empty size".to_string());
    }
    if t == "unlimited" || t == "none" || t == "0" {
        return Ok(u64::MAX);
    }
    let (num, mult) = if let Some(n) = t.strip_suffix("gib") {
        (n, 1024u64 * 1024 * 1024)
    } else if let Some(n) = t.strip_suffix("mib") {
        (n, 1024 * 1024)
    } else if let Some(n) = t.strip_suffix("kib") {
        (n, 1024)
    } else if let Some(n) = t.strip_suffix("gb") {
        (n, 1_000_000_000)
    } else if let Some(n) = t.strip_suffix("mb") {
        (n, 1_000_000)
    } else if let Some(n) = t.strip_suffix("kb") {
        (n, 1_000)
    } else if let Some(n) = t.strip_suffix('g') {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = t.strip_suffix('m') {
        (n, 1024 * 1024)
    } else if let Some(n) = t.strip_suffix('k') {
        (n, 1024)
    } else if let Some(n) = t.strip_suffix('b') {
        (n, 1)
    } else {
        (t.as_str(), 1)
    };

    let value: f64 = num
        .parse()
        .map_err(|_| format!("not a size: {s} (try 5MiB, 800KB or a plain byte count)"))?;
    if value < 0.0 {
        return Err(format!("negative size: {s}"));
    }
    Ok((value * mult as f64).round() as u64)
}

/// Render a byte count the way the report shows it.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes == u64::MAX {
        return "unlimited".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Shorten a list of locations for a one-line report, keeping the count
/// honest: "a, b, c and 7 more".
pub fn summarize_list(items: &[String], keep: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    if items.len() <= keep {
        return items.join(", ");
    }
    format!(
        "{} and {} more",
        items[..keep].join(", "),
        items.len() - keep
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hrefs_resolve_against_the_referring_document() {
        assert_eq!(
            resolve("OEBPS/text/p001.xhtml", "../images/a.png"),
            "OEBPS/images/a.png"
        );
        assert_eq!(
            resolve("OEBPS/content.opf", "text/p001.xhtml"),
            "OEBPS/text/p001.xhtml"
        );
        assert_eq!(resolve("content.opf", "images/a.png"), "images/a.png");
        assert_eq!(
            resolve("OEBPS/text/p.xhtml", "/images/a.png"),
            "images/a.png"
        );
        // Fragments and queries are not part of the resource path.
        assert_eq!(
            resolve("OEBPS/nav.xhtml", "text/p001.xhtml#top"),
            "OEBPS/text/p001.xhtml"
        );
        // A percent-encoded name matches the entry it was encoded from.
        assert_eq!(
            resolve("OEBPS/c.opf", "images/page%2001.png"),
            "OEBPS/images/page 01.png"
        );
    }

    #[test]
    fn remote_hrefs_are_recognized() {
        assert!(is_remote("https://example.com/a.png"));
        assert!(is_remote("//example.com/a.png"));
        assert!(is_remote("data:image/png;base64,AAAA"));
        assert!(!is_remote("../images/a.png"));
    }

    #[test]
    fn sizes_accept_binary_and_decimal_units() {
        assert_eq!(parse_size("1024").unwrap(), 1024);
        assert_eq!(parse_size("5MiB").unwrap(), 5 * 1024 * 1024);
        assert_eq!(parse_size("800KB").unwrap(), 800_000);
        assert_eq!(parse_size("1.5MiB").unwrap(), 1_572_864);
        assert_eq!(parse_size("unlimited").unwrap(), u64::MAX);
        assert!(parse_size("later").is_err());
    }

    #[test]
    fn byte_counts_stay_readable() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.0 KiB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(human_bytes(u64::MAX), "unlimited");
    }

    #[test]
    fn long_lists_are_summarized_without_lying_about_the_count() {
        let items: Vec<String> = (1..=10).map(|n| format!("p{n}")).collect();
        assert_eq!(summarize_list(&items, 3), "p1, p2, p3 and 7 more");
        assert_eq!(summarize_list(&items[..2], 3), "p1, p2");
    }
}
