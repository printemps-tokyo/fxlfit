//! Intrinsic image size without decoding.
//!
//! The tool needs pixel dimensions to tell an upscaled page from a sharp one,
//! and nothing else about the pixels. So headers are read and the image data
//! is left alone: no decoder runs over the bytes of an EPUB that arrived from
//! somewhere else, and a 12000 x 18000 page costs the same as a thumbnail.

/// Sniff the pixel size of an image from its leading bytes.
///
/// Returns `None` for a format that is not recognized or a header that is
/// truncated -- the caller reports that as unknown rather than guessing.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return png(bytes);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return jpeg(bytes);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return gif(bytes);
    }
    if bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        return webp(bytes);
    }
    None
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

/// IHDR is required to be the first chunk, so width and height sit at a fixed
/// offset. (PNG 3rd edition, 11.2.2 IHDR.)
fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[12..16] != b"IHDR" {
        return None;
    }
    Some((be32(&bytes[16..20]), be32(&bytes[20..24])))
}

/// Walk the JPEG marker segments to the frame header; the size lives in the
/// SOFn payload, which can sit behind a large EXIF or ICC segment.
fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2usize;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // Standalone markers carry no length field.
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((w, h));
        }
        // Entropy-coded data follows the scan header; nothing after it is a
        // frame header for the purposes of this sniff.
        if marker == 0xDA {
            return None;
        }
        if len < 2 {
            return None;
        }
        i += 2 + len;
    }
    None
}

fn gif(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 10 {
        return None;
    }
    let w = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
    let h = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
    Some((w, h))
}

/// The three WebP chunk layouts: lossy (VP8 ), lossless (VP8L) and extended
/// (VP8X). Canvas size is read from whichever is present.
fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 {
        return None;
    }
    match &bytes[12..16] {
        b"VP8 " => {
            // Frame header: 3-byte tag, 3-byte start code, then 16-bit sizes
            // whose top two bits are scaling hints.
            if bytes.len() < 30 {
                return None;
            }
            let w = u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3FFF;
            let h = u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3FFF;
            Some((w, h))
        }
        b"VP8L" => {
            let b = &bytes[21..25];
            let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            Some((w, h))
        }
        b"VP8X" => {
            let w = 1 + (bytes[24] as u32 | (bytes[25] as u32) << 8 | (bytes[26] as u32) << 16);
            let h = 1 + (bytes[27] as u32 | (bytes[28] as u32) << 8 | (bytes[29] as u32) << 16);
            Some((w, h))
        }
        _ => None,
    }
}

/// Media type a store or reading system will see, derived from the bytes
/// rather than from the manifest, so a mislabelled resource is visible.
pub fn sniff_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 12 {
        return None;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF8") {
        Some("image/gif")
    } else if bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&w.to_be_bytes());
        out.extend_from_slice(&h.to_be_bytes());
        out.extend_from_slice(&[8, 2, 0, 0, 0]);
        out
    }

    #[test]
    fn png_dimensions_come_from_ihdr() {
        assert_eq!(dimensions(&png_header(1200, 1800)), Some((1200, 1800)));
    }

    #[test]
    fn jpeg_dimensions_survive_a_large_leading_segment() {
        let mut bytes = vec![0xFF, 0xD8];
        // An APP1 segment big enough to push the frame header past any naive
        // fixed-offset read.
        let payload = vec![0u8; 4096];
        bytes.extend_from_slice(&[0xFF, 0xE1]);
        bytes.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        bytes.extend_from_slice(&payload);
        // SOF0: precision, height, width, components.
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&1800u16.to_be_bytes());
        bytes.extend_from_slice(&1200u16.to_be_bytes());
        bytes.extend_from_slice(&[0x03]);
        bytes.extend_from_slice(&[0u8; 16]);
        assert_eq!(dimensions(&bytes), Some((1200, 1800)));
    }

    #[test]
    fn gif_dimensions_are_little_endian() {
        let mut bytes = b"GIF89a".to_vec();
        bytes.extend_from_slice(&800u16.to_le_bytes());
        bytes.extend_from_slice(&1280u16.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 8]);
        assert_eq!(dimensions(&bytes), Some((800, 1280)));
    }

    #[test]
    fn webp_vp8x_carries_the_canvas_size() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0]); // flags and reserved
                                                // Canvas size is stored as width-1 and height-1, 24 bits each.
        bytes.extend_from_slice(&[0xAF, 0x04, 0x00]); // 1199 + 1 = 1200
        bytes.extend_from_slice(&[0x0F, 0x07, 0x00]); // 1807 + 1 = 1808
        assert_eq!(dimensions(&bytes), Some((1200, 1808)));
    }

    #[test]
    fn an_unknown_or_truncated_header_is_unknown_rather_than_wrong() {
        assert_eq!(dimensions(b"not an image at all"), None);
        assert_eq!(dimensions(&[0x89, b'P', b'N', b'G']), None);
    }

    #[test]
    fn media_types_are_sniffed_from_the_bytes() {
        assert_eq!(sniff_media_type(&png_header(1, 1)), Some("image/png"));
        assert_eq!(sniff_media_type(b"GIF89a01234567"), Some("image/gif"));
        assert_eq!(sniff_media_type(b"<svg xmlns=...>"), None);
    }
}
