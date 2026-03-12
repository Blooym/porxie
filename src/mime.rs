use mime::Mime;

/// Sniff the MIME type from the given bytes, returning `application/octet-stream` if unknown.
pub fn sniff_mime(buf: &[u8]) -> Mime {
    // WORKAROUND: infer does not correctly detect all SVG variants.
    // This case is handled manually until the upstream crate is modified.
    const SVG_MARKER: &[u8; 4] = b"<svg";
    const XML_MARKER: &[u8; 5] = b"<?xml";
    if buf.starts_with(SVG_MARKER)
        || (buf.starts_with(XML_MARKER)
            && buf
                .get(..256)
                .unwrap_or(buf)
                .windows(4)
                .any(|w| w == SVG_MARKER))
    {
        return mime::IMAGE_SVG;
    }

    match infer::get(buf) {
        Some(m) => m
            .mime_type()
            .parse()
            .expect("infer mimetype should always be valid"),
        None => mime::APPLICATION_OCTET_STREAM,
    }
}

pub fn is_mime_allowed(mime: &Mime, allowed: &[Mime]) -> bool {
    const STAR: &str = "*";

    if allowed.is_empty() {
        return false;
    }

    for allowed_mime in allowed {
        // MIME is '*/*', allow everything.
        if allowed_mime.type_() == STAR && allowed_mime.subtype() == STAR {
            return true;
        }

        // MIME subtype is *, allow if the type matches.
        if allowed_mime.subtype() == STAR && allowed_mime.type_() == mime.type_() {
            return true;
        }

        // Check if the mimes are exactly equal.
        if mime == allowed_mime {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use mime::Mime;
    use std::str::FromStr;

    #[test]
    fn test_is_mime_allowed() {
        // Test PNG when nothing is allowed.
        assert_eq!(
            super::is_mime_allowed(&Mime::from_str("image/png").unwrap(), &vec![]),
            false
        );

        // Test PNG when PNG is allowed.
        assert_eq!(
            super::is_mime_allowed(
                &Mime::from_str("image/png").unwrap(),
                &vec![mime::IMAGE_PNG],
            ),
            true
        );

        // Test PNG when only JPG is allowed.
        assert_eq!(
            super::is_mime_allowed(
                &Mime::from_str("image/png").unwrap(),
                &vec![mime::IMAGE_JPEG],
            ),
            false
        );

        // Test PNG when any image subtype is allowed.
        assert_eq!(
            super::is_mime_allowed(
                &Mime::from_str("image/png").unwrap(),
                &vec![mime::IMAGE_STAR],
            ),
            true
        );

        // Test PNG when anything is allowed.
        assert_eq!(
            super::is_mime_allowed(
                &Mime::from_str("image/png").unwrap(),
                &vec![mime::STAR_STAR],
            ),
            true
        );

        // Test HTML when any image subtype is enabled.
        assert_eq!(
            super::is_mime_allowed(
                &Mime::from_str("text/html").unwrap(),
                &vec![mime::IMAGE_STAR],
            ),
            false
        );

        // Test PNG when images and text are enabled.
        assert_eq!(
            super::is_mime_allowed(
                &Mime::from_str("image/png").unwrap(),
                &vec![mime::TEXT_STAR, mime::IMAGE_STAR],
            ),
            true
        );
    }
}
