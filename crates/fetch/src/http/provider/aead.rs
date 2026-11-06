//! Record protection: AES-GCM and ChaCha20-Poly1305 for TLS 1.2 and 1.3. Ported from
//! `rustls-rustcrypto` 0.0.2-alpha `src/aead.rs` (MIT OR Apache-2.0; see `provider.rs`).

pub(super) mod chacha20;
pub(super) mod gcm;

use aes_gcm::aead;
use rustls::crypto::cipher::{
    BorrowedPayload, InboundOpaqueMessage, InboundPlainMessage, MessageDecrypter, MessageEncrypter, OutboundOpaqueMessage, OutboundPlainMessage,
    PrefixedPayload,
};

/// The tag every one of these ciphers appends.
const TAG_LEN: usize = 16;

pub(crate) struct EncryptBufferAdapter<'a>(pub(crate) &'a mut PrefixedPayload);

impl AsRef<[u8]> for EncryptBufferAdapter<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl AsMut<[u8]> for EncryptBufferAdapter<'_> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}

impl aead::Buffer for EncryptBufferAdapter<'_> {
    fn extend_from_slice(&mut self, other: &[u8]) -> aead::Result<()> {
        self.0.extend_from_slice(other);
        Ok(())
    }

    fn truncate(&mut self, len: usize) {
        self.0.truncate(len)
    }
}

pub(crate) struct DecryptBufferAdapter<'a, 'p>(pub(crate) &'a mut BorrowedPayload<'p>);

impl AsRef<[u8]> for DecryptBufferAdapter<'_, '_> {
    fn as_ref(&self) -> &[u8] {
        self.0
    }
}

impl AsMut<[u8]> for DecryptBufferAdapter<'_, '_> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0
    }
}

impl aead::Buffer for DecryptBufferAdapter<'_, '_> {
    /// Decrypting in place only shrinks the buffer (the original said `unreachable!`).
    fn extend_from_slice(&mut self, _: &[u8]) -> aead::Result<()> {
        Err(aead::Error)
    }

    fn truncate(&mut self, len: usize) {
        self.0.truncate(len)
    }
}

/// The cipher made from a key or IV of the wrong length (rustls never passes one): it refuses
/// every record, so the connection fails instead of the program.
pub(crate) struct Refuse;

impl MessageEncrypter for Refuse {
    fn encrypt(&mut self, _: OutboundPlainMessage<'_>, _: u64) -> Result<OutboundOpaqueMessage, rustls::Error> {
        Err(rustls::Error::EncryptError)
    }

    fn encrypted_payload_len(&self, payload_len: usize) -> usize {
        payload_len
    }
}

impl MessageDecrypter for Refuse {
    fn decrypt<'a>(&mut self, _: InboundOpaqueMessage<'a>, _: u64) -> Result<InboundPlainMessage<'a>, rustls::Error> {
        Err(rustls::Error::DecryptError)
    }
}
