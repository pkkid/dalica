//! AES-128-GCM and AES-256-GCM records. Ported from `rustls-rustcrypto` 0.0.2-alpha
//! `src/aead/gcm.rs` (MIT OR Apache-2.0; see `provider.rs`), with the lengths checked and the
//! key-extraction variant named per cipher.

use aes_gcm::aead::{AeadInPlace, KeyInit, KeySizeUser};
use rustls::crypto::cipher::{
    self, AeadKey, InboundOpaqueMessage, InboundPlainMessage, Iv, KeyBlockShape, MessageDecrypter, MessageEncrypter, OutboundOpaqueMessage,
    OutboundPlainMessage, PrefixedPayload, Tls12AeadAlgorithm, Tls13AeadAlgorithm, UnsupportedOperationError,
};
use rustls::{ConnectionTrafficSecrets, ContentType, ProtocolVersion};

use super::{DecryptBufferAdapter, EncryptBufferAdapter, Refuse, TAG_LEN};

/// TLS 1.2 GCM: the 4-byte salt from the key block, then 8 explicit nonce bytes in each record.
const TLS12_FIXED_IV_LEN: usize = 4;
const TLS12_EXPLICIT_NONCE_LEN: usize = 8;
const TLS12_OVERHEAD: usize = TLS12_EXPLICIT_NONCE_LEN + TAG_LEN;

/// The 12-byte nonce of TLS 1.2 GCM: the salt, then the explicit part.
fn tls12_nonce(salt: &[u8], explicit: &[u8]) -> Option<[u8; 12]> {
    let salt: [u8; TLS12_FIXED_IV_LEN] = salt.try_into().ok()?;
    let explicit: [u8; TLS12_EXPLICIT_NONCE_LEN] = explicit.try_into().ok()?;
    let mut iv = [0u8; 12];
    iv[..TLS12_FIXED_IV_LEN].copy_from_slice(&salt);
    iv[TLS12_FIXED_IV_LEN..].copy_from_slice(&explicit);
    Some(iv)
}

macro_rules! impl_gcm {
    ($tls13:ident, $tls13_cipher:ident, $tls12:ident, $tls12_enc:ident, $tls12_dec:ident, $aead:ty, $secrets:ident) => {
        pub(in crate::http::provider) struct $tls13;

        impl Tls13AeadAlgorithm for $tls13 {
            fn encrypter(&self, key: AeadKey, iv: Iv) -> Box<dyn MessageEncrypter> {
                match <$aead>::new_from_slice(key.as_ref()) {
                    Ok(aead) => Box::new($tls13_cipher(aead, iv)),
                    Err(_) => Box::new(Refuse),
                }
            }

            fn decrypter(&self, key: AeadKey, iv: Iv) -> Box<dyn MessageDecrypter> {
                match <$aead>::new_from_slice(key.as_ref()) {
                    Ok(aead) => Box::new($tls13_cipher(aead, iv)),
                    Err(_) => Box::new(Refuse),
                }
            }

            fn key_len(&self) -> usize {
                <$aead>::key_size()
            }

            fn extract_keys(&self, key: AeadKey, iv: Iv) -> Result<ConnectionTrafficSecrets, UnsupportedOperationError> {
                Ok(ConnectionTrafficSecrets::$secrets { key, iv })
            }
        }

        struct $tls13_cipher($aead, Iv);

        impl MessageEncrypter for $tls13_cipher {
            fn encrypt(&mut self, m: OutboundPlainMessage<'_>, seq: u64) -> Result<OutboundOpaqueMessage, rustls::Error> {
                let total_len = self.encrypted_payload_len(m.payload.len());
                let mut payload = PrefixedPayload::with_capacity(total_len);
                let nonce = cipher::Nonce::new(&self.1, seq).0;
                let aad = cipher::make_tls13_aad(total_len);
                payload.extend_from_chunks(&m.payload);
                payload.extend_from_slice(&m.typ.to_array());
                self.0
                    .encrypt_in_place(&nonce.into(), &aad, &mut EncryptBufferAdapter(&mut payload))
                    .map_err(|_| rustls::Error::EncryptError)
                    .map(|()| OutboundOpaqueMessage::new(ContentType::ApplicationData, ProtocolVersion::TLSv1_2, payload))
            }

            fn encrypted_payload_len(&self, payload_len: usize) -> usize {
                payload_len.saturating_add(1 + TAG_LEN)
            }
        }

        impl MessageDecrypter for $tls13_cipher {
            fn decrypt<'a>(&mut self, mut m: InboundOpaqueMessage<'a>, seq: u64) -> Result<InboundPlainMessage<'a>, rustls::Error> {
                let payload = &mut m.payload;
                let nonce = cipher::Nonce::new(&self.1, seq).0;
                let aad = cipher::make_tls13_aad(payload.len());
                self.0.decrypt_in_place(&nonce.into(), &aad, &mut DecryptBufferAdapter(payload)).map_err(|_| rustls::Error::DecryptError)?;
                m.into_tls13_unpadded_message()
            }
        }

        pub(in crate::http::provider) struct $tls12;

        impl Tls12AeadAlgorithm for $tls12 {
            fn encrypter(&self, key: AeadKey, write_iv: &[u8], explicit: &[u8]) -> Box<dyn MessageEncrypter> {
                match (<$aead>::new_from_slice(key.as_ref()), tls12_nonce(write_iv, explicit)) {
                    (Ok(aead), Some(iv)) => Box::new($tls12_enc(aead, Iv::from(iv))),
                    _ => Box::new(Refuse),
                }
            }

            fn decrypter(&self, key: AeadKey, iv: &[u8]) -> Box<dyn MessageDecrypter> {
                match (<$aead>::new_from_slice(key.as_ref()), <[u8; TLS12_FIXED_IV_LEN]>::try_from(iv)) {
                    (Ok(aead), Ok(salt)) => Box::new($tls12_dec(aead, salt)),
                    _ => Box::new(Refuse),
                }
            }

            fn key_block_shape(&self) -> KeyBlockShape {
                KeyBlockShape { enc_key_len: <$aead>::key_size(), fixed_iv_len: TLS12_FIXED_IV_LEN, explicit_nonce_len: TLS12_EXPLICIT_NONCE_LEN }
            }

            fn extract_keys(&self, key: AeadKey, iv: &[u8], explicit: &[u8]) -> Result<ConnectionTrafficSecrets, UnsupportedOperationError> {
                let iv = tls12_nonce(iv, explicit).ok_or(UnsupportedOperationError)?;
                Ok(ConnectionTrafficSecrets::$secrets { key, iv: Iv::new(iv) })
            }
        }

        struct $tls12_enc($aead, Iv);

        impl MessageEncrypter for $tls12_enc {
            fn encrypt(&mut self, m: OutboundPlainMessage<'_>, seq: u64) -> Result<OutboundOpaqueMessage, rustls::Error> {
                let total_len = self.encrypted_payload_len(m.payload.len());
                let mut payload = PrefixedPayload::with_capacity(total_len);
                let nonce = cipher::Nonce::new(&self.1, seq).0;
                let aad = cipher::make_tls12_aad(seq, m.typ, m.version, m.payload.len());
                payload.extend_from_slice(nonce.get(TLS12_FIXED_IV_LEN..).unwrap_or_default());
                payload.extend_from_chunks(&m.payload);
                let body = payload.as_mut().get_mut(TLS12_EXPLICIT_NONCE_LEN..).ok_or(rustls::Error::EncryptError)?;
                let tag = self.0.encrypt_in_place_detached(&nonce.into(), &aad, body).map_err(|_| rustls::Error::EncryptError)?;
                payload.extend_from_slice(tag.as_ref());
                Ok(OutboundOpaqueMessage::new(m.typ, m.version, payload))
            }

            fn encrypted_payload_len(&self, payload_len: usize) -> usize {
                payload_len.saturating_add(TLS12_OVERHEAD)
            }
        }

        struct $tls12_dec($aead, [u8; TLS12_FIXED_IV_LEN]);

        impl MessageDecrypter for $tls12_dec {
            fn decrypt<'a>(&mut self, mut m: InboundOpaqueMessage<'a>, seq: u64) -> Result<InboundPlainMessage<'a>, rustls::Error> {
                let len = m.payload.len();
                let Some(plain_len) = len.checked_sub(TLS12_OVERHEAD) else { return Err(rustls::Error::DecryptError) };
                let explicit = m.payload.get(..TLS12_EXPLICIT_NONCE_LEN).ok_or(rustls::Error::DecryptError)?;
                let nonce = tls12_nonce(&self.1, explicit).ok_or(rustls::Error::DecryptError)?;
                let aad = cipher::make_tls12_aad(seq, m.typ, m.version, plain_len);
                {
                    let body = m.payload.get_mut(TLS12_EXPLICIT_NONCE_LEN..).ok_or(rustls::Error::DecryptError)?;
                    let (msg, tag) = body.split_at_mut_checked(plain_len).ok_or(rustls::Error::DecryptError)?;
                    let tag = aes_gcm::Tag::from_exact_iter(tag.iter().copied()).ok_or(rustls::Error::DecryptError)?;
                    self.0.decrypt_in_place_detached(&nonce.into(), &aad, msg, &tag).map_err(|_| rustls::Error::DecryptError)?;
                }
                // moved only after a good tag, so a failed record is left as it was
                m.payload.rotate_left(TLS12_EXPLICIT_NONCE_LEN);
                m.payload.truncate(plain_len);
                Ok(m.into_plain_message())
            }
        }
    };
}

impl_gcm!(Tls13Aes128Gcm, Tls13CipherAes128Gcm, Tls12Aes128Gcm, Tls12EncAes128Gcm, Tls12DecAes128Gcm, aes_gcm::Aes128Gcm, Aes128Gcm);
impl_gcm!(Tls13Aes256Gcm, Tls13CipherAes256Gcm, Tls12Aes256Gcm, Tls12EncAes256Gcm, Tls12DecAes256Gcm, aes_gcm::Aes256Gcm, Aes256Gcm);
