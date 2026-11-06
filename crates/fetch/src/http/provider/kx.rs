//! Key exchange: X25519, P-256 and P-384 ECDHE. Ported from `rustls-rustcrypto` 0.0.2-alpha
//! `src/kx.rs` (MIT OR Apache-2.0; see `provider.rs`).

use rustls::crypto::{ActiveKeyExchange, SharedSecret, SupportedKxGroup};
use rustls::{NamedGroup, PeerMisbehaved};

fn invalid_share() -> rustls::Error {
    rustls::Error::from(PeerMisbehaved::InvalidKeyShare)
}

#[derive(Debug)]
struct X25519;

impl SupportedKxGroup for X25519 {
    fn name(&self) -> NamedGroup {
        NamedGroup::X25519
    }

    fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, rustls::Error> {
        let priv_key = x25519_dalek::EphemeralSecret::random_from_rng(rand_core::OsRng);
        let pub_key = (&priv_key).into();
        Ok(Box::new(X25519KeyExchange { priv_key, pub_key }))
    }
}

struct X25519KeyExchange {
    priv_key: x25519_dalek::EphemeralSecret,
    pub_key: x25519_dalek::PublicKey,
}

impl ActiveKeyExchange for X25519KeyExchange {
    fn complete(self: Box<X25519KeyExchange>, peer: &[u8]) -> Result<SharedSecret, rustls::Error> {
        let peer: [u8; 32] = peer.try_into().map_err(|_| invalid_share())?;
        let shared = self.priv_key.diffie_hellman(&peer.into());
        // a low-order point gives an all-zero secret: refused, as RFC 7748 §6.1 allows (and
        // `ring` does); the original accepted it
        if !shared.was_contributory() {
            return Err(invalid_share());
        }
        Ok(shared.as_bytes()[..].into())
    }

    fn pub_key(&self) -> &[u8] {
        self.pub_key.as_bytes()
    }

    fn group(&self) -> NamedGroup {
        NamedGroup::X25519
    }
}

macro_rules! impl_kx {
    ($name:ident, $exchange:ident, $group:expr, $secret:ty, $public_key:ty) => {
        #[derive(Debug)]
        struct $name;

        impl SupportedKxGroup for $name {
            fn name(&self) -> NamedGroup {
                $group
            }

            fn start(&self) -> Result<Box<dyn ActiveKeyExchange>, rustls::Error> {
                let priv_key = <$secret>::random(&mut rand_core::OsRng);
                let pub_key: $public_key = (&priv_key).into();
                Ok(Box::new($exchange { priv_key, pub_key: pub_key.to_sec1_bytes() }))
            }
        }

        struct $exchange {
            priv_key: $secret,
            pub_key: Box<[u8]>,
        }

        impl ActiveKeyExchange for $exchange {
            fn complete(self: Box<$exchange>, peer: &[u8]) -> Result<SharedSecret, rustls::Error> {
                let theirs = <$public_key>::from_sec1_bytes(peer).map_err(|_| invalid_share())?;
                Ok(self.priv_key.diffie_hellman(&theirs).raw_secret_bytes().as_slice().into())
            }

            fn pub_key(&self) -> &[u8] {
                &self.pub_key
            }

            fn group(&self) -> NamedGroup {
                $group
            }
        }
    };
}

impl_kx!(SecP256R1, SecP256R1KeyExchange, NamedGroup::secp256r1, p256::ecdh::EphemeralSecret, p256::PublicKey);
impl_kx!(SecP384R1, SecP384R1KeyExchange, NamedGroup::secp384r1, p384::ecdh::EphemeralSecret, p384::PublicKey);

/// Every group, in the order the client offers them.
pub(super) const ALL_KX_GROUPS: &[&dyn SupportedKxGroup] = &[&X25519, &SecP256R1, &SecP384R1];
