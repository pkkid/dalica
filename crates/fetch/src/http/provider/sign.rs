//! Signing keys: RSA (PKCS#1 v1.5 and PSS), P-256/P-384 ECDSA and Ed25519. The client never
//! signs (no client certificate); a `CryptoProvider` needs a key provider, and the tests' local
//! server signs with it. Ported from `rustls-rustcrypto` 0.0.2-alpha `src/sign.rs` and
//! `src/sign/{rsa,ecdsa,eddsa}.rs` (MIT OR Apache-2.0; see `provider.rs`), without its SEC1
//! reading of an Ed25519 key (no such encoding exists).

use std::marker::PhantomData;
use std::sync::Arc;

use p256::pkcs8::DecodePrivateKey;
use rsa::RsaPrivateKey;
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::signature::{RandomizedSigner, SignatureEncoding};
use rustls::pki_types::PrivateKeyDer;
use rustls::sign::{Signer, SigningKey};
use rustls::{SignatureAlgorithm, SignatureScheme};
use sha2::{Sha256, Sha384, Sha512};

fn refused(what: &str) -> rustls::Error {
    rustls::Error::General(format!("the private key: {what}"))
}

/// Any key this provider can sign with: RSA, then ECDSA P-256 and P-384, then Ed25519.
pub(super) fn any_supported_type(der: &PrivateKeyDer<'_>) -> Result<Arc<dyn SigningKey>, rustls::Error> {
    if let Ok(k) = RsaSigningKey::try_from(der) {
        return Ok(Arc::new(k));
    }
    if let Ok(k) = EcdsaSigningKeyP256::try_from(der) {
        return Ok(Arc::new(k));
    }
    if let Ok(k) = EcdsaSigningKeyP384::try_from(der) {
        return Ok(Arc::new(k));
    }
    Ed25519SigningKey::try_from(der).map(|k| Arc::new(k) as Arc<dyn SigningKey>)
}

/// Signs with a signer that needs random numbers (RSA, ECDSA), making signatures of type `S`.
struct RandomizedSigning<S, T> {
    key: Arc<T>,
    scheme: SignatureScheme,
    _sig: PhantomData<fn() -> S>,
}

impl<S, T> RandomizedSigning<S, T> {
    fn boxed(key: Arc<T>, scheme: SignatureScheme) -> Box<dyn Signer>
    where
        T: RandomizedSigner<S> + Send + Sync + 'static,
        S: SignatureEncoding + 'static,
    {
        Box::new(RandomizedSigning { key, scheme, _sig: PhantomData })
    }
}

impl<S, T> std::fmt::Debug for RandomizedSigning<S, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RandomizedSigning").field("scheme", &self.scheme).finish_non_exhaustive()
    }
}

impl<S, T> Signer for RandomizedSigning<S, T>
where
    T: RandomizedSigner<S> + Send + Sync,
    S: SignatureEncoding,
{
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.key
            .try_sign_with_rng(&mut rand_core::OsRng, message)
            .map(|sig: S| sig.to_vec())
            .map_err(|_| rustls::Error::General("signing failed".into()))
    }

    fn scheme(&self) -> SignatureScheme {
        self.scheme
    }
}

#[derive(Debug)]
struct RsaSigningKey(RsaPrivateKey);

impl TryFrom<&PrivateKeyDer<'_>> for RsaSigningKey {
    type Error = rustls::Error;

    fn try_from(value: &PrivateKeyDer<'_>) -> Result<Self, Self::Error> {
        let key = match value {
            PrivateKeyDer::Pkcs8(der) => RsaPrivateKey::from_pkcs8_der(der.secret_pkcs8_der()).map_err(|e| refused(&e.to_string()))?,
            PrivateKeyDer::Pkcs1(der) => RsaPrivateKey::from_pkcs1_der(der.secret_pkcs1_der()).map_err(|e| refused(&e.to_string()))?,
            _ => return Err(refused("not an RSA key")),
        };
        Ok(Self(key))
    }
}

/// The RSA schemes, most preferred first.
const RSA_SCHEMES: &[SignatureScheme] = &[
    SignatureScheme::RSA_PSS_SHA512,
    SignatureScheme::RSA_PSS_SHA384,
    SignatureScheme::RSA_PSS_SHA256,
    SignatureScheme::RSA_PKCS1_SHA512,
    SignatureScheme::RSA_PKCS1_SHA384,
    SignatureScheme::RSA_PKCS1_SHA256,
];

impl SigningKey for RsaSigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        let scheme = *RSA_SCHEMES.iter().find(|s| offered.contains(s))?;
        let key = self.0.clone();
        Some(match scheme {
            SignatureScheme::RSA_PSS_SHA512 => {
                RandomizedSigning::<rsa::pss::Signature, _>::boxed(Arc::new(rsa::pss::SigningKey::<Sha512>::new(key)), scheme)
            }
            SignatureScheme::RSA_PSS_SHA384 => {
                RandomizedSigning::<rsa::pss::Signature, _>::boxed(Arc::new(rsa::pss::SigningKey::<Sha384>::new(key)), scheme)
            }
            SignatureScheme::RSA_PSS_SHA256 => {
                RandomizedSigning::<rsa::pss::Signature, _>::boxed(Arc::new(rsa::pss::SigningKey::<Sha256>::new(key)), scheme)
            }
            SignatureScheme::RSA_PKCS1_SHA512 => {
                RandomizedSigning::<rsa::pkcs1v15::Signature, _>::boxed(Arc::new(rsa::pkcs1v15::SigningKey::<Sha512>::new(key)), scheme)
            }
            SignatureScheme::RSA_PKCS1_SHA384 => {
                RandomizedSigning::<rsa::pkcs1v15::Signature, _>::boxed(Arc::new(rsa::pkcs1v15::SigningKey::<Sha384>::new(key)), scheme)
            }
            SignatureScheme::RSA_PKCS1_SHA256 => {
                RandomizedSigning::<rsa::pkcs1v15::Signature, _>::boxed(Arc::new(rsa::pkcs1v15::SigningKey::<Sha256>::new(key)), scheme)
            }
            _ => return None,
        })
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::RSA
    }
}

macro_rules! ecdsa_key {
    ($ty:ident, $curve:ident, $scheme:expr) => {
        #[derive(Debug)]
        struct $ty(Arc<$curve::ecdsa::SigningKey>);

        impl TryFrom<&PrivateKeyDer<'_>> for $ty {
            type Error = rustls::Error;

            fn try_from(value: &PrivateKeyDer<'_>) -> Result<Self, Self::Error> {
                let key = match value {
                    PrivateKeyDer::Pkcs8(der) => {
                        $curve::ecdsa::SigningKey::from_pkcs8_der(der.secret_pkcs8_der()).map_err(|e| refused(&e.to_string()))?
                    }
                    PrivateKeyDer::Sec1(der) => {
                        let secret = $curve::SecretKey::from_sec1_der(der.secret_sec1_der()).map_err(|e| refused(&e.to_string()))?;
                        $curve::ecdsa::SigningKey::from(secret)
                    }
                    _ => return Err(refused("not an ECDSA key")),
                };
                Ok(Self(Arc::new(key)))
            }
        }

        impl SigningKey for $ty {
            fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
                offered.contains(&$scheme).then(|| RandomizedSigning::<$curve::ecdsa::DerSignature, _>::boxed(Arc::clone(&self.0), $scheme))
            }

            fn algorithm(&self) -> SignatureAlgorithm {
                SignatureAlgorithm::ECDSA
            }
        }
    };
}

ecdsa_key!(EcdsaSigningKeyP256, p256, SignatureScheme::ECDSA_NISTP256_SHA256);
ecdsa_key!(EcdsaSigningKeyP384, p384, SignatureScheme::ECDSA_NISTP384_SHA384);

#[derive(Debug)]
struct Ed25519SigningKey(Arc<ed25519_dalek::SigningKey>);

impl TryFrom<&PrivateKeyDer<'_>> for Ed25519SigningKey {
    type Error = rustls::Error;

    fn try_from(value: &PrivateKeyDer<'_>) -> Result<Self, Self::Error> {
        match value {
            PrivateKeyDer::Pkcs8(der) => {
                ed25519_dalek::SigningKey::from_pkcs8_der(der.secret_pkcs8_der()).map(|k| Self(Arc::new(k))).map_err(|e| refused(&e.to_string()))
            }
            _ => Err(refused("not an Ed25519 key")),
        }
    }
}

/// Signs with Ed25519 (deterministic).
#[derive(Debug)]
struct Ed25519Signing(Arc<ed25519_dalek::SigningKey>);

impl Signer for Ed25519Signing {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        use ed25519_dalek::Signer as _;
        self.0.try_sign(message).map(|sig| sig.to_vec()).map_err(|_| rustls::Error::General("signing failed".into()))
    }

    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}

impl SigningKey for Ed25519SigningKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered.contains(&SignatureScheme::ED25519).then(|| Box::new(Ed25519Signing(Arc::clone(&self.0))) as Box<dyn Signer>)
    }

    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ED25519
    }
}
