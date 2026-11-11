//! Recognize mislabeled SSE without waiting for the upstream to finish.
use super::*;

pub(super) async fn event_stream(
    headers: &HeaderMap,
    source: &mut UpstreamBodyStream,
) -> Result<bool, BoxError> {
    // Standard passthrough may contain compressed bytes; only inspect decoded
    // bodies. A compressed JSON body must retain its encoding and exact bytes.
    if headers
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .any(|v| v != "identity")
    {
        return Ok(false);
    }
    let mut prefix = Vec::new();
    let detected = loop {
        if let Some(detected) = classify(&prefix) {
            break detected;
        }
        // Bound a whitespace-only prelude; detection never needs a full event.
        if prefix.len() >= 1024 {
            break false;
        }
        match source.next().await {
            Some(Ok(bytes)) => prefix.extend_from_slice(&bytes),
            Some(Err(error)) => return Err(error),
            None => break false,
        }
    };
    if !prefix.is_empty() {
        let rest = std::mem::replace(source, Box::pin(stream::empty()));
        *source = Box::pin(stream::once(async move { Ok(Bytes::from(prefix)) }).chain(rest));
    }
    Ok(detected)
}

fn classify(prefix: &[u8]) -> Option<bool> {
    const BOM: &[u8] = b"\xef\xbb\xbf";
    if prefix.len() < BOM.len() && BOM.starts_with(prefix) {
        return None;
    }
    let prefix = prefix.strip_prefix(BOM).unwrap_or(prefix);
    let prefix = prefix.trim_ascii_start();
    let fields: [&[u8]; 5] = [b"data:", b"event:", b"id:", b"retry:", b":"];
    if fields.iter().any(|field| prefix.starts_with(field)) {
        Some(true)
    } else if fields.iter().any(|field| field.starts_with(prefix)) {
        None
    } else {
        Some(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sniffing_is_chunk_invariant_lossless_and_bounded() {
        for (body, expected) in [
            ("data: {}\n\n", true),
            ("\u{feff}\r\nevent: message_start\ndata: {}\n\n", true),
            (": keepalive\n\n", true),
            ("id: 1\nretry: 1000\n", true),
            ("{\"data\":\"event: text\"}", false),
            ("<html>error</html>", false),
            ("dat", false),
        ] {
            for split in 0..=body.len() {
                let bytes = body.as_bytes();
                let chunks = vec![
                    Ok(Bytes::copy_from_slice(&bytes[..split])),
                    Ok(Bytes::copy_from_slice(&bytes[split..])),
                ];
                let mut source: UpstreamBodyStream = Box::pin(stream::iter(chunks));
                assert_eq!(
                    event_stream(&HeaderMap::new(), &mut source).await.unwrap(),
                    expected,
                    "{body:?} split {split}"
                );
                let mut restored = Vec::new();
                while let Some(bytes) = source.next().await {
                    restored.extend_from_slice(&bytes.unwrap());
                }
                assert_eq!(restored, bytes);
            }
        }
        let mut source: UpstreamBodyStream = Box::pin(
            stream::once(async { Ok(Bytes::from(vec![b' '; 1024])) }).chain(stream::pending()),
        );
        assert!(!tokio::time::timeout(
            Duration::from_secs(1),
            event_stream(&HeaderMap::new(), &mut source)
        )
        .await
        .unwrap()
        .unwrap());
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        let mut source: UpstreamBodyStream = Box::pin(stream::pending());
        assert!(
            !tokio::time::timeout(Duration::from_secs(1), event_stream(&headers, &mut source))
                .await
                .unwrap()
                .unwrap()
        );
    }
}
