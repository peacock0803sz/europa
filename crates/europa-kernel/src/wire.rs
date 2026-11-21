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
