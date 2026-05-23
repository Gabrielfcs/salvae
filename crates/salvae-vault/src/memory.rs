//! In-memory Channel implementation for tests and downstream crates.
//!
//! Messages get monotonically increasing ids (1, 2, 3, ...). Listing returns
//! newest-first, matching the Discord semantics the vault relies on.

use std::cell::RefCell;

use crate::channel::{AttachmentRef, Channel, Message, MessageId};
use crate::VaultError;

struct StoredMessage {
    id: MessageId,
    content: String,
    /// (filename, bytes) for each attachment, in order.
    attachments: Vec<(String, Vec<u8>)>,
}

/// A simple in-memory `Channel`. Uses interior mutability so it satisfies the
/// `&self` trait methods.
#[derive(Default)]
pub struct InMemoryChannel {
    messages: RefCell<Vec<StoredMessage>>,
    next_id: RefCell<MessageId>,
}

impl InMemoryChannel {
    pub fn new() -> Self {
        Self { messages: RefCell::new(Vec::new()), next_id: RefCell::new(1) }
    }

    /// Number of messages currently stored (test helper).
    pub fn len(&self) -> usize {
        self.messages.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Channel for InMemoryChannel {
    fn list_messages(
        &self,
        before: Option<MessageId>,
        limit: u16,
    ) -> Result<Vec<Message>, VaultError> {
        let messages = self.messages.borrow();
        let mut out: Vec<Message> = messages
            .iter()
            .rev() // newest-first
            .filter(|m| match before {
                Some(b) => m.id < b,
                None => true,
            })
            .take(limit as usize)
            .map(|m| Message {
                id: m.id,
                content: m.content.clone(),
                attachments: m
                    .attachments
                    .iter()
                    .enumerate()
                    .map(|(i, (filename, _))| AttachmentRef { id: i as u64, filename: filename.clone() })
                    .collect(),
            })
            .collect();
        out.shrink_to_fit();
        Ok(out)
    }

    fn send_message(
        &self,
        content: &str,
        attachments: &[(String, Vec<u8>)],
    ) -> Result<Message, VaultError> {
        let id = {
            let mut next = self.next_id.borrow_mut();
            let id = *next;
            *next += 1;
            id
        };
        self.messages.borrow_mut().push(StoredMessage {
            id,
            content: content.to_string(),
            attachments: attachments.to_vec(),
        });
        Ok(Message {
            id,
            content: content.to_string(),
            attachments: attachments
                .iter()
                .enumerate()
                .map(|(i, (filename, _))| AttachmentRef { id: i as u64, filename: filename.clone() })
                .collect(),
        })
    }

    fn download_attachment(
        &self,
        message_id: MessageId,
        attachment: &AttachmentRef,
    ) -> Result<Vec<u8>, VaultError> {
        let messages = self.messages.borrow();
        let msg = messages.iter().find(|m| m.id == message_id).ok_or(VaultError::NotFound)?;
        let found = msg
            .attachments
            .iter()
            .find(|(filename, _)| *filename == attachment.filename)
            .ok_or(VaultError::NotFound)?;
        Ok(found.1.clone())
    }

    fn delete_message(&self, message_id: MessageId) -> Result<(), VaultError> {
        let mut messages = self.messages.borrow_mut();
        let before = messages.len();
        messages.retain(|m| m.id != message_id);
        if messages.len() == before {
            return Err(VaultError::NotFound);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_then_list_returns_newest_first() {
        let ch = InMemoryChannel::new();
        ch.send_message("first", &[]).unwrap();
        ch.send_message("second", &[]).unwrap();
        let all = ch.list_messages(None, 100).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].content, "second");
        assert_eq!(all[1].content, "first");
        assert!(all[0].id > all[1].id);
    }

    #[test]
    fn limit_and_before_paginate() {
        let ch = InMemoryChannel::new();
        let m1 = ch.send_message("a", &[]).unwrap();
        let _m2 = ch.send_message("b", &[]).unwrap();
        let m3 = ch.send_message("c", &[]).unwrap();

        let first_page = ch.list_messages(None, 2).unwrap();
        assert_eq!(first_page.len(), 2);
        assert_eq!(first_page[0].id, m3.id);

        // Everything older than m3 (i.e. m1, m2), newest-first.
        let older = ch.list_messages(Some(m3.id), 100).unwrap();
        assert_eq!(older.len(), 2);
        assert_eq!(older[1].id, m1.id);
    }

    #[test]
    fn attachments_round_trip_through_download() {
        let ch = InMemoryChannel::new();
        let msg = ch
            .send_message("withfile", &[("chunk_0.bin".into(), vec![1, 2, 3, 4])])
            .unwrap();
        assert_eq!(msg.attachments.len(), 1);
        let bytes = ch.download_attachment(msg.id, &msg.attachments[0]).unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4]);
    }

    #[test]
    fn delete_removes_message() {
        let ch = InMemoryChannel::new();
        let m = ch.send_message("bye", &[]).unwrap();
        ch.delete_message(m.id).unwrap();
        assert!(ch.is_empty());
    }

    #[test]
    fn download_missing_message_is_not_found() {
        let ch = InMemoryChannel::new();
        let att = AttachmentRef { id: 0, filename: "x".into() };
        assert!(matches!(ch.download_attachment(999, &att), Err(VaultError::NotFound)));
    }

    #[test]
    fn delete_missing_message_is_not_found() {
        let ch = InMemoryChannel::new();
        assert!(matches!(ch.delete_message(999), Err(VaultError::NotFound)));
    }
}
