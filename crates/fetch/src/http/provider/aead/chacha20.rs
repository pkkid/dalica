//! ChaCha20-Poly1305 records. Ported from `rustls-rustcrypto` 0.0.2-alpha `src/aead/chacha20.rs`
//! (MIT OR Apache-2.0; see `provider.rs`), with the lengths checked: a TLS 1.2 record shorter
//! than the tag is refused (the original subtracted and could wrap).

use chacha20poly1305::{AeadInPlace, ChaCha20Poly1305, KeyInit, KeySizeUser};
use rustls::crypto::cipher::{
    self, AeadKey, InboundOpaqueMessage, InboundPlainMessage, Iv, KeyBlockShape, MessageDecrypter, MessageEncrypter, NONCE_LEN,
    OutboundOpaqueMessage, OutboundPlainMessage, PrefixedPayload, Tls12AeadAlgorithm, Tls13AeadAlgorithm, UnsupportedOperationError,
};
use rustls::{ConnectionTrafficSecrets, ContentType, ProtocolVersion};

use super::{DecryptBufferAdapter, EncryptBufferAdapter, Refuse, TAG_LEN};

pub(in crate::http::provider) struct Chacha20Poly1305;

/// The cipher for `key` and `iv`, or one that refuses every record when either has the wrong length.
fn cipher_for<T: 'static>(key: &AeadKey, iv: Option<Iv>, make: fn(ChaCha20Poly1305, Iv) -> T) -> Option<T> {
    let aead = ChaCha20Poly1305::new_from_slice(key.as_ref()).ok()?;
    Some(make(aead, iv?))
}

fn iv12(iv: &[u8]) -> Option<Iv> {
    <[u8; NONCE_LEN]>::try_from(iv).ok().map(Iv::new)
}

impl Tls13AeadAlgorithm for Chacha20Poly1305 {
    fn encrypter(&self, key: AeadKey, iv: Iv) -> Box<dyn MessageEncrypter> {
        match cipher_for(&key, Some(iv), Tls13Cipher) {
            Some(c) => Box::new(c),
            None => Box::new(Refuse),
        }
    }

    fn decrypter(&self, key: AeadKey, iv: Iv) -> Box<dyn MessageDecrypter> {
        match cipher_for(&key, Some(iv), Tls13Cipher) {
            Some(c) => Box::new(c),
            None => Box::new(Refuse),
        }
    }

    fn key_len(&self) -> usize {
        ChaCha20Poly1305::key_size()
    }

    fn extract_keys(&self, key: AeadKey, iv: Iv) -> Result<ConnectionTrafficSecrets, UnsupportedOperationError> {
        Ok(ConnectionTrafficSecrets::Chacha20Poly1305 { key, iv })
    }
}

impl Tls12AeadAlgorithm for Chacha20Poly1305 {
    fn encrypter(&self, key: AeadKey, iv: &[u8], _: &[u8]) -> Box<dyn MessageEncrypter> {
        match cipher_for(&key, iv12(iv), Tls12Cipher) {
            Some(c) => Box::new(c),
            None => Box::new(Refuse),
        }
    }

    fn decrypter(&self, key: AeadKey, iv: &[u8]) -> Box<dyn MessageDecrypter> {
        match cipher_for(&key, iv12(iv), Tls12Cipher) {
            Some(c) => Box::new(c),
            None => Box::new(Refuse),
        }
    }

    fn key_block_shape(&self) -> KeyBlockShape {
        KeyBlockShape { enc_key_len: 32, fixed_iv_len: 12, explicit_nonce_len: 0 }
    }

    fn extract_keys(&self, key: AeadKey, iv: &[u8], _explicit: &[u8]) -> Result<ConnectionTrafficSecrets, UnsupportedOperationError> {
        Ok(ConnectionTrafficSecrets::Chacha20Poly1305 { key, iv: iv12(iv).ok_or(UnsupportedOperationError)? })
    }
}

struct Tls13Cipher(ChaCha20Poly1305, Iv);

impl MessageEncrypter for Tls13Cipher {
    fn encrypt(&mut self, m: OutboundPlainMessage<'_>, seq: u64) -> Result<OutboundOpaqueMessage, rustls::Error> {
        let total_len = self.encrypted_payload_len(m.payload.len());
        let mut payload = PrefixedPayload::with_capacity(total_len);
        payload.extend_from_chunks(&m.payload);
        payload.extend_from_slice(&m.typ.to_array());
        let nonce = cipher::Nonce::new(&self.1, seq).0;
        let aad = cipher::make_tls13_aad(total_len);
        self.0
            .encrypt_in_place(&nonce.into(), &aad, &mut EncryptBufferAdapter(&mut payload))
            .map_err(|_| rustls::Error::EncryptError)
            .map(|()| OutboundOpaqueMessage::new(ContentType::ApplicationData, ProtocolVersion::TLSv1_2, payload))
    }

    fn encrypted_payload_len(&self, payload_len: usize) -> usize {
        payload_len.saturating_add(1 + TAG_LEN)
    }
}

impl MessageDecrypter for Tls13Cipher {
    fn decrypt<'a>(&mut self, mut m: InboundOpaqueMessage<'a>, seq: u64) -> Result<InboundPlainMessage<'a>, rustls::Error> {
        let payload = &mut m.payload;
        let nonce = cipher::Nonce::new(&self.1, seq).0;
        let aad = cipher::make_tls13_aad(payload.len());
        self.0.decrypt_in_place(&nonce.into(), &aad, &mut DecryptBufferAdapter(payload)).map_err(|_| rustls::Error::DecryptError)?;
        m.into_tls13_unpadded_message()
    }
}

struct Tls12Cipher(ChaCha20Poly1305, Iv);

impl MessageEncrypter for Tls12Cipher {
    fn encrypt(&mut self, m: OutboundPlainMessage<'_>, seq: u64) -> Result<OutboundOpaqueMessage, rustls::Error> {
        let total_len = self.encrypted_payload_len(m.payload.len());
        let mut payload = PrefixedPayload::with_capacity(total_len);
        payload.extend_from_chunks(&m.payload);
        let nonce = cipher::Nonce::new(&self.1, seq).0;
        let aad = cipher::make_tls12_aad(seq, m.typ, m.version, m.payload.len());
        self.0
            .encrypt_in_place(&nonce.into(), &aad, &mut EncryptBufferAdapter(&mut payload))
            .map_err(|_| rustls::Error::EncryptError)
            .map(|()| OutboundOpaqueMessage::new(m.typ, m.version, payload))
    }

    fn encrypted_payload_len(&self, payload_len: usize) -> usize {
        payload_len.saturating_add(TAG_LEN)
    }
}

impl MessageDecrypter for Tls12Cipher {
    fn decrypt<'a>(&mut self, mut m: InboundOpaqueMessage<'a>, seq: u64) -> Result<InboundPlainMessage<'a>, rustls::Error> {
        let plain_len = m.payload.len().checked_sub(TAG_LEN).ok_or(rustls::Error::DecryptError)?;
        let nonce = cipher::Nonce::new(&self.1, seq).0;
        let aad = cipher::make_tls12_aad(seq, m.typ, m.version, plain_len);
        self.0.decrypt_in_place(&nonce.into(), &aad, &mut DecryptBufferAdapter(&mut m.payload)).map_err(|_| rustls::Error::DecryptError)?;
        Ok(m.into_plain_message())
    }
}
