//! Jupyter wire message framing and HMAC signing.
//!
//! The functions here only pack and unpack frames; they never touch sockets.
//! Signing covers the header, parent header, metadata, and content JSON in
//! that order, following the Jupyter wire protocol.

/// Header of a wire message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Header {
    /// Message id of this message.
    pub(crate) msg_id: String,
    /// Session id shared by one client.
    pub(crate) session: String,
    /// User name, always `europa` for messages we send.
    pub(crate) username: String,
    /// UTC timestamp in ISO 8601 form.
    pub(crate) date: String,
    /// Jupyter message type such as `execute_request`.
    pub(crate) msg_type: String,
    /// Protocol version such as `5.3`.
    pub(crate) version: String,
}

/// One wire message with its routing identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireMessage {
    /// Routing identities placed before `<IDS|MSG>`.
    pub(crate) identities: Vec<Vec<u8>>,
    /// Header of this message.
    pub(crate) header: Header,
    /// Parent header, `None` when the sender left `{}`.
    pub(crate) parent_header: Option<Header>,
    /// Metadata JSON.
    pub(crate) metadata: serde_json::Value,
    /// Content JSON.
    pub(crate) content: serde_json::Value,
    /// Binary buffers after the content.
    pub(crate) buffers: Vec<Vec<u8>>,
}

/// Why decoding a wire message failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DecodeError {
    /// The HMAC signature did not match.
    BadSignature,
    /// The frames could not be read, with a reason.
    Malformed(String),
}

/// Compute the HMAC-SHA256 signature of message parts as lowercase hex.
///
/// An empty key disables signing and yields an empty string.
pub(crate) fn sign(key: &[u8], parts: &[&[u8]]) -> String {
    let _ = (key, parts);
    todo!()
}

/// Pack a message into frames for sending.
pub(crate) fn encode(msg: &WireMessage, key: &[u8]) -> Vec<Vec<u8>> {
    let _ = (msg, key);
    todo!()
}

/// Unpack frames into a message, checking the signature first.
pub(crate) fn decode(frames: &[Vec<u8>], key: &[u8]) -> Result<WireMessage, DecodeError> {
    let _ = (frames, key);
    todo!()
}

/// Make a random version 4 UUID string.
pub(crate) fn new_id() -> String {
    todo!()
}

/// Build a header for an outgoing message.
pub(crate) fn header(msg_type: &str, session: &str) -> Header {
    let _ = (msg_type, session);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> WireMessage {
        WireMessage {
            identities: vec![b"id-a".to_vec(), b"id-b".to_vec()],
            header: Header {
                msg_id: "aaa".to_owned(),
                session: "sess".to_owned(),
                username: "europa".to_owned(),
                date: "2026-01-02T03:04:05.000006Z".to_owned(),
                msg_type: "execute_request".to_owned(),
                version: "5.3".to_owned(),
            },
            parent_header: Some(Header {
                msg_id: "ppp".to_owned(),
                session: "sess".to_owned(),
                username: "europa".to_owned(),
                date: "2026-01-02T03:04:05.000007Z".to_owned(),
                msg_type: "execute_request".to_owned(),
                version: "5.3".to_owned(),
            }),
            metadata: serde_json::json!({}),
            content: serde_json::json!({"code": "1+1"}),
            buffers: vec![b"buf".to_vec()],
        }
    }

    #[test]
    fn signs_split_parts_like_one_part() {
        assert_eq!(
            sign(b"Jefe", &[b"what do ya ", b"want for nothing?"]),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn empty_key_disables_signing() {
        assert_eq!(sign(b"", &[b"anything"]), "");
    }

    #[test]
    fn round_trips_frames_in_order() {
        let key = b"secret";
        let msg = sample();
        let frames = encode(&msg, key);
        let mut at = msg.identities.len();
        assert_eq!(frames[at], b"<IDS|MSG>".to_vec());
        at += 1;
        assert!(!frames[at].is_empty(), "signature is present");
        at += 1;
        let header: Header = serde_json::from_slice(&frames[at]).expect("header reads back");
        assert_eq!(header, msg.header);
        at += 1;
        let parent: Header = serde_json::from_slice(&frames[at]).expect("parent reads back");
        assert_eq!(Some(parent), msg.parent_header);
        assert_eq!(decode(&frames, key), Ok(msg));
    }

    #[test]
    fn writes_missing_parent_as_empty_object() {
        let key = b"secret";
        let mut msg = sample();
        msg.parent_header = None;
        let frames = encode(&msg, key);
        let at = msg.identities.len() + 3;
        assert_eq!(frames[at], b"{}".to_vec());
        assert_eq!(decode(&frames, key), Ok(msg));
    }

    #[test]
    fn changed_content_fails_the_signature() {
        let key = b"secret";
        let mut frames = encode(&sample(), key);
        let content_at = sample().identities.len() + 5;
        let byte = frames[content_at][0];
        frames[content_at][0] = byte.wrapping_add(1);
        assert_eq!(decode(&frames, key), Err(DecodeError::BadSignature));
    }

    #[test]
    fn empty_key_passes_without_a_signature() {
        let mut msg = sample();
        msg.parent_header = None;
        let frames = encode(&msg, b"");
        assert_eq!(decode(&frames, b""), Ok(msg));
    }

    #[test]
    fn rejects_frames_without_a_delimiter_or_header() {
        let key = b"secret";
        let frames = encode(&sample(), key);
        let without_marker: Vec<Vec<u8>> = frames
            .iter()
            .filter(|frame| frame.as_slice() != b"<IDS|MSG>")
            .cloned()
            .collect();
        assert!(matches!(
            decode(&without_marker, key),
            Err(DecodeError::Malformed(_))
        ));
        assert!(matches!(
            decode(&frames[..2], key),
            Err(DecodeError::Malformed(_))
        ));
        let mut broken = frames.clone();
        let header_at = sample().identities.len() + 2;
        broken[header_at] = b"not json".to_vec();
        // Re-sign the broken header so the failure is the parse, not the
        // signature.
        let signature_at = sample().identities.len() + 1;
        let parts: Vec<&[u8]> = broken
            .iter()
            .skip(signature_at + 1)
            .take(4)
            .map(Vec::as_slice)
            .collect();
        broken[signature_at] = sign(key, &parts).into_bytes();
        assert!(matches!(
            decode(&broken, key),
            Err(DecodeError::Malformed(_))
        ));
    }

    #[test]
    fn makes_uuid_v4_strings() {
        let first = new_id();
        let second = new_id();
        for id in [&first, &second] {
            assert_eq!(id.len(), 36);
            assert_eq!(id.as_bytes()[14], b'4');
            assert!(matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        }
        assert_ne!(first, second);
    }

    #[test]
    fn builds_headers_with_utc_dates() {
        let made = header("execute_request", "s");
        assert_eq!(made.msg_type, "execute_request");
        assert_eq!(made.session, "s");
        assert_eq!(made.username, "europa");
        assert_eq!(made.version, "5.3");
        let date = made.date.as_bytes();
        assert_eq!(date.len(), 27);
        assert_eq!(date[4], b'-');
        assert_eq!(date[7], b'-');
        assert_eq!(date[10], b'T');
        assert_eq!(date[13], b':');
        assert_eq!(date[16], b':');
        assert_eq!(date[19], b'.');
        assert_eq!(date[26], b'Z');
    }
}
