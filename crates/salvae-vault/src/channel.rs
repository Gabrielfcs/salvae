//! Transport interface and message data types.
//!
//! A `Channel` is an ordered append-style message store where each message has
//! text content and zero or more named binary attachments. The vault uses it
//! without knowing whether it is backed by Discord, memory, or anything else.

use crate::VaultError;

/// Identifier of a stored message. Larger ids are newer (matching Discord's
/// snowflake ordering and the in-memory fake's counter).
pub type MessageId = u64;

/// A reference to one attachment on a message (not its bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentRef {
    /// Transport-specific attachment id (Discord attachment id; index in the fake).
    pub id: u64,
    /// Original file name (the vault uses `chunk_{i}.bin`).
    pub filename: String,
}

/// A stored message: text content plus attachment references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub content: String,
    pub attachments: Vec<AttachmentRef>,
}

/// An ordered message store with binary attachments.
pub trait Channel {
    /// Return up to `limit` messages newest-first. If `before` is `Some(id)`,
    /// only messages with an id strictly less than `id` are returned (for
    /// pagination). `limit` should be treated as a maximum (Discord caps at 100).
    fn list_messages(
        &self,
        before: Option<MessageId>,
        limit: u16,
    ) -> Result<Vec<Message>, VaultError>;

    /// Create a message with `content` and the given named binary attachments.
    /// Returns the created message (with assigned id and attachment refs).
    fn send_message(
        &self,
        content: &str,
        attachments: &[(String, Vec<u8>)],
    ) -> Result<Message, VaultError>;

    /// Download the raw bytes of `attachment` belonging to `message_id`.
    fn download_attachment(
        &self,
        message_id: MessageId,
        attachment: &AttachmentRef,
    ) -> Result<Vec<u8>, VaultError>;

    /// Permanently delete a message.
    fn delete_message(&self, message_id: MessageId) -> Result<(), VaultError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_and_attachment_are_constructible_and_comparable() {
        let a = Message {
            id: 7,
            content: "hi".into(),
            attachments: vec![AttachmentRef { id: 0, filename: "chunk_0.bin".into() }],
        };
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(a.attachments[0].filename, "chunk_0.bin");
    }
}
