//! The TLS crypto provider (`security.md` §8.2): RustCrypto's AES-GCM, ChaCha20-Poly1305, X25519,
//! P-256/P-384 key exchange, SHA-256/384 and HMAC, and P-256/P-384 ECDSA, Ed25519 and RSA
//! signatures, in pure Rust (no `ring`, no assembly, no C build). The client uses it to verify the
//! server's signatures; signing (`KeyProvider`) is here because a `CryptoProvider` needs one, and
//! the tests' local server uses it.
//!
//! Ported from `rustls-rustcrypto` 0.0.2-alpha (github.com/RustCrypto/rustls-rustcrypto,
//! `src/lib.rs`, `aead.rs`, `aead/{gcm,chacha20}.rs`, `hash.rs`, `hmac.rs`, `kx.rs`, `sign.rs`,
//! `sign/*.rs`, `verify.rs`, `verify/*.rs`; MIT OR Apache-2.0, Copyright the RustCrypto
//! Developers), changed for Dalica: the signature algorithm identifiers come from
//! `rustls::pki_types::alg_id` instead of its own `rustls-webpki` 0.102 copy (M16.17); no
//! `unwrap`, `expect`, `unreachable!` or `unsafe` (a key of the wrong length makes a cipher that
//! refuses every record instead of a panic, lengths are checked before they are subtracted, and
//! the cipher suite lists are written out instead of concatenated with `transmute`); no `paste`
//! (the macros take every name); its QUIC keys, `no_std` switches and the SEC1-encoded Ed25519
//! key are left out; `extract_keys` names the cipher it was asked about; and an X25519 share that
//! gives an all-zero secret is refused.

mod aead;
mod hash;
mod kx;
mod sign;
mod verify;

use std::sync::Arc;

use rustls::crypto::tls12::PrfUsingHmac;
use rustls::crypto::tls13::HkdfUsingHmac;
use rustls::crypto::{CipherSuiteCommon, CryptoProvider, GetRandomFailed, KeyExchangeAlgorithm, KeyProvider, SecureRandom};
use rustls::pki_types::PrivateKeyDer;
use rustls::{CipherSuite, SignatureScheme, SupportedCipherSuite, Tls12CipherSuite, Tls13CipherSuite};

/// The provider: every cipher suite below, X25519 then P-256 then P-384 key exchange, the
/// signature algorithms of [`verify::ALGORITHMS`], the operating system's random numbers.
pub fn provider() -> CryptoProvider {
    CryptoProvider {
        cipher_suites: ALL_CIPHER_SUITES.to_vec(),
        kx_groups: kx::ALL_KX_GROUPS.to_vec(),
        signature_verification_algorithms: verify::ALGORITHMS,
        secure_random: &Provider,
        key_provider: &Provider,
    }
}

#[derive(Debug)]
struct Provider;

impl SecureRandom for Provider {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), GetRandomFailed> {
        use rand_core::RngCore;
        rand_core::OsRng.try_fill_bytes(bytes).map_err(|_| GetRandomFailed)
    }
}

impl KeyProvider for Provider {
    fn load_private_key(&self, key_der: PrivateKeyDer<'static>) -> Result<Arc<dyn rustls::sign::SigningKey>, rustls::Error> {
        sign::any_supported_type(&key_der)
    }
}

const TLS12_ECDSA_SCHEMES: &[SignatureScheme] = &[
    SignatureScheme::ECDSA_NISTP256_SHA256,
    SignatureScheme::ECDSA_NISTP384_SHA384,
    SignatureScheme::ECDSA_NISTP521_SHA512,
    SignatureScheme::ED25519,
];

const TLS12_RSA_SCHEMES: &[SignatureScheme] = &[
    SignatureScheme::RSA_PKCS1_SHA256,
    SignatureScheme::RSA_PKCS1_SHA384,
    SignatureScheme::RSA_PKCS1_SHA512,
    SignatureScheme::RSA_PSS_SHA256,
    SignatureScheme::RSA_PSS_SHA384,
    SignatureScheme::RSA_PSS_SHA512,
];

/// A TLS 1.2 ECDHE suite.
macro_rules! tls12_suite {
    ($suite:ident, $hash:expr, $hmac:expr, $sign:expr, $aead:expr) => {
        SupportedCipherSuite::Tls12(&Tls12CipherSuite {
            common: CipherSuiteCommon { suite: CipherSuite::$suite, hash_provider: $hash, confidentiality_limit: u64::MAX },
            kx: KeyExchangeAlgorithm::ECDHE,
            sign: $sign,
            aead_alg: $aead,
            prf_provider: &PrfUsingHmac($hmac),
        })
    };
}

/// A TLS 1.3 suite.
macro_rules! tls13_suite {
    ($suite:ident, $hash:expr, $hmac:expr, $aead:expr) => {
        SupportedCipherSuite::Tls13(&Tls13CipherSuite {
            common: CipherSuiteCommon { suite: CipherSuite::$suite, hash_provider: $hash, confidentiality_limit: u64::MAX },
            hkdf_provider: &HkdfUsingHmac($hmac),
            aead_alg: $aead,
            quic: None,
        })
    };
}

/// Every suite, in the original's order: TLS 1.2's ECDSA then RSA suites, then TLS 1.3's (a
/// TLS 1.3 connection uses only the TLS 1.3 ones).
pub const ALL_CIPHER_SUITES: &[SupportedCipherSuite] = &[
    tls12_suite!(TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256, hash::SHA256, hash::HMAC_SHA256, TLS12_ECDSA_SCHEMES, &aead::gcm::Tls12Aes128Gcm),
    tls12_suite!(TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384, hash::SHA384, hash::HMAC_SHA384, TLS12_ECDSA_SCHEMES, &aead::gcm::Tls12Aes256Gcm),
    tls12_suite!(
        TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
        hash::SHA256,
        hash::HMAC_SHA256,
        TLS12_ECDSA_SCHEMES,
        &aead::chacha20::Chacha20Poly1305
    ),
    tls12_suite!(TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256, hash::SHA256, hash::HMAC_SHA256, TLS12_RSA_SCHEMES, &aead::gcm::Tls12Aes128Gcm),
    tls12_suite!(TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384, hash::SHA384, hash::HMAC_SHA384, TLS12_RSA_SCHEMES, &aead::gcm::Tls12Aes256Gcm),
    tls12_suite!(TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256, hash::SHA256, hash::HMAC_SHA256, TLS12_RSA_SCHEMES, &aead::chacha20::Chacha20Poly1305),
    tls13_suite!(TLS13_AES_128_GCM_SHA256, hash::SHA256, hash::HMAC_SHA256, &aead::gcm::Tls13Aes128Gcm),
    tls13_suite!(TLS13_AES_256_GCM_SHA384, hash::SHA384, hash::HMAC_SHA384, &aead::gcm::Tls13Aes256Gcm),
    tls13_suite!(TLS13_CHACHA20_POLY1305_SHA256, hash::SHA256, hash::HMAC_SHA256, &aead::chacha20::Chacha20Poly1305),
];
