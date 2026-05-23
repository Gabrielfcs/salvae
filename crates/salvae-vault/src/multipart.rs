//! Pure multipart/form-data body builder (Discord upload format).
//!
//! Discord's "create message" endpoint expects a `payload_json` field plus one
//! `files[i]` field per attachment. This module builds that body as raw bytes;
//! it performs no IO and is used by the live Discord transport (separate plan).

/// The `Content-Type` header value for a body built with `boundary`.
pub fn content_type(boundary: &str) -> String {
    format!("multipart/form-data; boundary={boundary}")
}

/// Build a multipart/form-data body: a `payload_json` part followed by one
/// `files[i]` part per `(filename, bytes)` in `files`, in order.
pub fn build_form_data(boundary: &str, payload_json: &str, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    let crlf = b"\r\n";

    // payload_json part
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"payload_json\"\r\n");
    body.extend_from_slice(b"Content-Type: application/json\r\n");
    body.extend_from_slice(crlf);
    body.extend_from_slice(payload_json.as_bytes());
    body.extend_from_slice(crlf);

    // one part per file
    for (i, (filename, bytes)) in files.iter().enumerate() {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"files[{i}]\"; filename=\"{filename}\"\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(b"Content-Type: application/octet-stream\r\n");
        body.extend_from_slice(crlf);
        body.extend_from_slice(bytes);
        body.extend_from_slice(crlf);
    }

    // closing boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_type_includes_boundary() {
        assert_eq!(content_type("ABC"), "multipart/form-data; boundary=ABC");
    }

    #[test]
    fn body_contains_payload_and_file_parts() {
        let files = vec![("chunk_0.bin".to_string(), vec![1u8, 2, 3])];
        let body = build_form_data("BOUND", "{\"content\":\"\"}", &files);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("--BOUND\r\n"));
        assert!(text.contains("name=\"payload_json\""));
        assert!(text.contains("{\"content\":\"\"}"));
        assert!(text.contains("name=\"files[0]\"; filename=\"chunk_0.bin\""));
        assert!(text.contains("application/octet-stream"));
        assert!(text.ends_with("--BOUND--\r\n"));
    }

    #[test]
    fn raw_file_bytes_are_embedded_verbatim() {
        let files = vec![("a.bin".to_string(), vec![0u8, 255, 10, 13])];
        let body = build_form_data("B", "{}", &files);
        // The 4 raw bytes must appear as a contiguous window in the body.
        assert!(body.windows(4).any(|w| w == [0u8, 255, 10, 13]));
    }

    #[test]
    fn no_files_still_produces_payload_and_closing_boundary() {
        let body = build_form_data("B", "{}", &[]);
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"payload_json\""));
        assert!(text.ends_with("--B--\r\n"));
        assert!(!text.contains("files["));
    }
}
