//! Signature verification: what the client checks the server's handshake signature and every
//! certificate's signature with. Ported from `rustls-rustcrypto` 0.0.2-alpha `src/verify.rs` and
//! `src/verify/{ecdsa,eddsa,rsa}.rs` (MIT OR Apache-2.0; see `provider.rs`), the algorithm
//! identifiers taken from `rustls::pki_types::alg_id` (the constants it took from its own
//! `rustls-webpki` 0.102 copy, byte for byte).

use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::signature::Verifier;
use rsa::{RsaPublicKey, pkcs1v15, pss};
use rustls::SignatureScheme;
use rustls::crypto::WebPkiSupportedAlgorithms;
use rustls::pki_types::{AlgorithmIdentifier, InvalidSignature, SignatureVerificationAlgorithm, alg_id};
use sha2::{Digest, Sha256, Sha384, Sha512};

/// Every algorithm, and which of them each TLS signature scheme may use.
pub(super) static ALGORITHMS: WebPkiSupportedAlgorithms = WebPkiSupportedAlgorithms {
    all: &[
        ECDSA_P256_SHA256,
        ECDSA_P256_SHA384,
        ECDSA_P384_SHA256,
        ECDSA_P384_SHA384,
        ED25519,
        RSA_PKCS1_SHA256,
        RSA_PKCS1_SHA384,
        RSA_PKCS1_SHA512,
        RSA_PSS_SHA256,
        RSA_PSS_SHA384,
        RSA_PSS_SHA512,
    ],
    mapping: &[
        (SignatureScheme::ECDSA_NISTP384_SHA384, &[ECDSA_P384_SHA384, ECDSA_P256_SHA384]),
        (SignatureScheme::ECDSA_NISTP256_SHA256, &[ECDSA_P256_SHA256, ECDSA_P384_SHA256]),
        (SignatureScheme::ED25519, &[ED25519]),
        (SignatureScheme::RSA_PKCS1_SHA256, &[RSA_PKCS1_SHA256]),
        (SignatureScheme::RSA_PKCS1_SHA384, &[RSA_PKCS1_SHA384]),
        (SignatureScheme::RSA_PKCS1_SHA512, &[RSA_PKCS1_SHA512]),
        (SignatureScheme::RSA_PSS_SHA256, &[RSA_PSS_SHA256]),
        (SignatureScheme::RSA_PSS_SHA384, &[RSA_PSS_SHA384]),
        (SignatureScheme::RSA_PSS_SHA512, &[RSA_PSS_SHA512]),
    ],
};

/// An ECDSA verifier: the curve's key, a DER signature over the message hashed with `$hash` (the
/// curves on the `p256`/`p384` 0.14 line since M16.29, RUSTSEC-2026-0387; the prehash is plain
/// bytes, so `sha2` 0.10 serves both lines).
macro_rules! ecdsa_verifier {
    ($name:ident, $ty:ident, $public_key_alg:expr, $signature_alg:expr, $curve:ident, $hash:ty) => {
        #[derive(Debug)]
        struct $ty;

        impl SignatureVerificationAlgorithm for $ty {
            fn public_key_alg_id(&self) -> AlgorithmIdentifier {
                $public_key_alg
            }

            fn signature_alg_id(&self) -> AlgorithmIdentifier {
                $signature_alg
            }

            fn verify_signature(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), InvalidSignature> {
                use $curve::ecdsa::signature::hazmat::PrehashVerifier as _;
                let signature = $curve::ecdsa::DerSignature::try_from(signature).map_err(|_| InvalidSignature)?;
                let key = $curve::ecdsa::VerifyingKey::from_sec1_bytes(public_key).map_err(|_| InvalidSignature)?;
                key.verify_prehash(&<$hash>::digest(message), &signature).map_err(|_| InvalidSignature)
            }
        }

        const $name: &dyn SignatureVerificationAlgorithm = &$ty;
    };
}

ecdsa_verifier!(ECDSA_P256_SHA256, EcdsaP256Sha256, alg_id::ECDSA_P256, alg_id::ECDSA_SHA256, p256, Sha256);
ecdsa_verifier!(ECDSA_P256_SHA384, EcdsaP256Sha384, alg_id::ECDSA_P256, alg_id::ECDSA_SHA384, p256, Sha384);
ecdsa_verifier!(ECDSA_P384_SHA256, EcdsaP384Sha256, alg_id::ECDSA_P384, alg_id::ECDSA_SHA256, p384, Sha256);
ecdsa_verifier!(ECDSA_P384_SHA384, EcdsaP384Sha384, alg_id::ECDSA_P384, alg_id::ECDSA_SHA384, p384, Sha384);

#[derive(Debug)]
struct Ed25519Verify;

impl SignatureVerificationAlgorithm for Ed25519Verify {
    fn public_key_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ED25519
    }

    fn signature_alg_id(&self) -> AlgorithmIdentifier {
        alg_id::ED25519
    }

    fn verify_signature(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), InvalidSignature> {
        let public_key: &[u8; 32] = public_key.try_into().map_err(|_| InvalidSignature)?;
        let signature = ed25519_dalek::Signature::from_slice(signature).map_err(|_| InvalidSignature)?;
        ed25519_dalek::VerifyingKey::from_bytes(public_key).map_err(|_| InvalidSignature)?.verify(message, &signature).map_err(|_| InvalidSignature)
    }
}

const ED25519: &dyn SignatureVerificationAlgorithm = &Ed25519Verify;

/// An RSA verifier: a PKCS#1 public key, a PKCS#1 v1.5 or PSS signature.
macro_rules! rsa_verifier {
    ($name:ident, $ty:ident, $signature_alg:expr, $verifying_key:ty, $signature:ty) => {
        #[derive(Debug)]
        struct $ty;

        impl SignatureVerificationAlgorithm for $ty {
            fn public_key_alg_id(&self) -> AlgorithmIdentifier {
                alg_id::RSA_ENCRYPTION
            }

            fn signature_alg_id(&self) -> AlgorithmIdentifier {
                $signature_alg
            }

            fn verify_signature(&self, public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<(), InvalidSignature> {
                let public_key = RsaPublicKey::from_pkcs1_der(public_key).map_err(|_| InvalidSignature)?;
                let signature = <$signature>::try_from(signature).map_err(|_| InvalidSignature)?;
                <$verifying_key>::new(public_key).verify(message, &signature).map_err(|_| InvalidSignature)
            }
        }

        const $name: &dyn SignatureVerificationAlgorithm = &$ty;
    };
}

rsa_verifier!(RSA_PKCS1_SHA256, RsaPkcs1Sha256, alg_id::RSA_PKCS1_SHA256, pkcs1v15::VerifyingKey<Sha256>, pkcs1v15::Signature);
rsa_verifier!(RSA_PKCS1_SHA384, RsaPkcs1Sha384, alg_id::RSA_PKCS1_SHA384, pkcs1v15::VerifyingKey<Sha384>, pkcs1v15::Signature);
rsa_verifier!(RSA_PKCS1_SHA512, RsaPkcs1Sha512, alg_id::RSA_PKCS1_SHA512, pkcs1v15::VerifyingKey<Sha512>, pkcs1v15::Signature);
rsa_verifier!(RSA_PSS_SHA256, RsaPssSha256, alg_id::RSA_PSS_SHA256, pss::VerifyingKey<Sha256>, pss::Signature);
rsa_verifier!(RSA_PSS_SHA384, RsaPssSha384, alg_id::RSA_PSS_SHA384, pss::VerifyingKey<Sha384>, pss::Signature);
rsa_verifier!(RSA_PSS_SHA512, RsaPssSha512, alg_id::RSA_PSS_SHA512, pss::VerifyingKey<Sha512>, pss::Signature);
