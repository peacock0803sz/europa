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

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// Compute the HMAC-SHA256 signature of message parts as lowercase hex.
///
/// An empty key disables signing and yields an empty string.
pub(crate) fn sign(key: &[u8], parts: &[&[u8]]) -> String {
    if key.is_empty() {
        return String::new();
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key size");
    for part in parts {
        mac.update(part);
    }
    let digest = mac.finalize().into_bytes();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(char::from_digit(u32::from(byte >> 4), 16).expect("hex digit"));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).expect("hex digit"));
    }
    out
}

/// Pack a message into frames for sending.
pub(crate) fn encode(msg: &WireMessage, key: &[u8]) -> Vec<Vec<u8>> {
    let header = serde_json::to_vec(&msg.header).expect("header serializes");
    let parent = match &msg.parent_header {
        Some(parent) => serde_json::to_vec(parent).expect("parent serializes"),
        None => b"{}".to_vec(),
    };
    let metadata = serde_json::to_vec(&msg.metadata).expect("metadata serializes");
    let content = serde_json::to_vec(&msg.content).expect("content serializes");
    let signature = sign(key, &[&header, &parent, &metadata, &content]);
    let mut frames = Vec::with_capacity(msg.identities.len() + 5 + msg.buffers.len());
    frames.extend(msg.identities.iter().cloned());
    frames.push(b"<IDS|MSG>".to_vec());
    frames.push(signature.into_bytes());
    frames.push(header);
    frames.push(parent);
    frames.push(metadata);
    frames.push(content);
    frames.extend(msg.buffers.iter().cloned());
    frames
}

/// Unpack frames into a message, checking the signature first.
pub(crate) fn decode(frames: &[Vec<u8>], key: &[u8]) -> Result<WireMessage, DecodeError> {
    let marker = frames
        .iter()
        .position(|frame| frame.as_slice() == b"<IDS|MSG>")
        .ok_or_else(|| DecodeError::Malformed("missing <IDS|MSG>".to_owned()))?;
    let rest = &frames[marker + 1..];
    if rest.len() < 5 {
        return Err(DecodeError::Malformed("too few frames".to_owned()));
    }
    let signature = &rest[0];
    let header_bytes = &rest[1];
    let parent_bytes = &rest[2];
    let metadata_bytes = &rest[3];
    let content_bytes = &rest[4];
    let buffers = rest[5..].to_vec();
    if !key.is_empty() {
        let expected = hex_decode(signature).ok_or(DecodeError::BadSignature)?;
        let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes any key size");
        mac.update(header_bytes);
        mac.update(parent_bytes);
        mac.update(metadata_bytes);
        mac.update(content_bytes);
        mac.verify_slice(&expected)
            .map_err(|_| DecodeError::BadSignature)?;
    }
    let header: Header = serde_json::from_slice(header_bytes)
        .map_err(|error| DecodeError::Malformed(error.to_string()))?;
    let parent_header = if parent_bytes.as_slice() == b"{}" {
        None
    } else {
        Some(
            serde_json::from_slice(parent_bytes)
                .map_err(|error| DecodeError::Malformed(error.to_string()))?,
        )
    };
    let metadata: serde_json::Value = serde_json::from_slice(metadata_bytes)
        .map_err(|error| DecodeError::Malformed(error.to_string()))?;
    let content: serde_json::Value = serde_json::from_slice(content_bytes)
        .map_err(|error| DecodeError::Malformed(error.to_string()))?;
    Ok(WireMessage {
        identities: frames[..marker].to_vec(),
        header,
        parent_header,
        metadata,
        content,
        buffers,
    })
}

/// Decode lowercase or uppercase hex into bytes.
fn hex_decode(text: &[u8]) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let (pairs, rest) = text.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in pairs {
        let high = hex_value(pair[0])?;
        let low = hex_value(pair[1])?;
        out.push((high << 4) | low);
    }
    Some(out)
}

/// Value of one hex digit.
fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// Make a random version 4 UUID string.
pub(crate) fn new_id() -> String {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).expect("randomness is available");
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: Vec<String> = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let flat: String = hex.concat();
    format!(
        "{}-{}-{}-{}-{}",
        &flat[0..8],
        &flat[8..12],
        &flat[12..16],
        &flat[16..20],
        &flat[20..32]
    )
}

/// Build a header for an outgoing message.
pub(crate) fn header(msg_type: &str, session: &str) -> Header {
    Header {
        msg_id: new_id(),
        session: session.to_owned(),
        username: "europa".to_owned(),
        date: utc_now(),
        msg_type: msg_type.to_owned(),
        version: "5.3".to_owned(),
    }
}

/// Current UTC time as `YYYY-MM-DDTHH:MM:SS.ffffffZ`.
fn utc_now() -> String {
    let now = std::time::SystemTime::now();
    let elapsed = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = elapsed.as_secs() as i64;
    let micros = elapsed.subsec_micros();
    let days = secs.div_euclid(86_400);
    let of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{micros:06}Z",
        of_day / 3_600,
        (of_day % 3_600) / 60,
        of_day % 60
    )
}

/// Convert days since the Unix epoch to a calendar date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let ordinal = shifted - era * 146_097;
    let year_of_era = (ordinal - ordinal / 1_460 + ordinal / 36_524 - ordinal / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = ordinal - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
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
