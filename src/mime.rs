use mime::Mime;

/// Sniff the MIME type from the given bytes.
///
/// Returns [`mime::APPLICATION_OCTET_STREAM`] when unknown.
#[must_use]
pub fn sniff_mime(buf: &[u8]) -> Mime {
    match infer::get(buf) {
        Some(m) => m
            .mime_type()
            .parse()
            .expect("infer mimetype should always be valid"),
        None => {
            // WORKAROUND: infer does not correctly detect SVG.
            // I have created PR to fix this at https://github.com/bojand/infer/pull/119
            // Until that is merged, this case will work around that limitation.
            const SVG_MARKER: &[u8; 4] = b"<svg";
            const XML_MARKER: &[u8; 5] = b"<?xml";
            const XML_SNIFFAHEAD: usize = 256; // How far after the XML marker to sniff ahead for the SVG marker.
            if buf.len() >= 4 && buf.starts_with(SVG_MARKER)
                || (buf.starts_with(XML_MARKER)
                    && buf
                        .get(..XML_SNIFFAHEAD)
                        .unwrap_or(buf)
                        .windows(SVG_MARKER.len())
                        .any(|w| w == SVG_MARKER))
            {
                tracing::debug!("used svg workaround instead of regular inference");
                return mime::IMAGE_SVG;
            }
            tracing::debug!("infer was unable to determine mimetype, using fallback value");
            mime::APPLICATION_OCTET_STREAM
        }
    }
}

/// Whether the given [`Mime`] is apart of the allowed array by
/// checking if it matches directly or by wildcard.
#[must_use]
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
    fn no_match() {
        // PNG when nothing is allowed.
        assert!(!super::is_mime_allowed(
            &Mime::from_str("image/png").unwrap(),
            &[]
        ));
    }

    #[test]
    fn exact_match() {
        // PNG when PNG is allowed.
        assert!(super::is_mime_allowed(
            &Mime::from_str("image/png").unwrap(),
            &[mime::IMAGE_PNG],
        ));

        // PNG when only JPG is allowed.
        assert!(!super::is_mime_allowed(
            &Mime::from_str("image/png").unwrap(),
            &[mime::IMAGE_JPEG],
        ));
    }

    #[test]
    fn full_wildcard() {
        // PNG when anything is allowed.
        assert!(super::is_mime_allowed(
            &Mime::from_str("image/png").unwrap(),
            &[mime::STAR_STAR],
        ));
    }

    #[test]
    fn subtype_wildcard() {
        // PNG when any image subtype is allowed.
        assert!(super::is_mime_allowed(
            &Mime::from_str("image/png").unwrap(),
            &[mime::IMAGE_STAR]
        ));

        // PNG when images and text are enabled.
        assert!(super::is_mime_allowed(
            &Mime::from_str("image/png").unwrap(),
            &[mime::TEXT_STAR, mime::IMAGE_STAR],
        ));

        // Test HTML when any image subtype is enabled.
        assert!(!super::is_mime_allowed(
            &Mime::from_str("text/html").unwrap(),
            &[mime::IMAGE_STAR],
        ));
    }
}
