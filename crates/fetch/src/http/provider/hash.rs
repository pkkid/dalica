//! SHA-256 and SHA-384, and HMAC over them. Ported from `rustls-rustcrypto` 0.0.2-alpha
//! `src/hash.rs` and `src/hmac.rs` (MIT OR Apache-2.0; see `provider.rs`).

use hmac::Mac;
use rustls::crypto::{self, hash};
use sha2::digest::OutputSizeUser;
use sha2::{Digest, Sha256, Sha384};

macro_rules! impl_hash {
    ($name:ident, $hash:ident, $context:ident, $ty:ty, $algo:expr) => {
        struct $hash;

        impl hash::Hash for $hash {
            fn start(&self) -> Box<dyn hash::Context> {
                Box::new($context(<$ty>::new()))
            }

            fn hash(&self, data: &[u8]) -> hash::Output {
                hash::Output::new(&<$ty>::digest(data)[..])
            }

            fn output_len(&self) -> usize {
                <$ty as OutputSizeUser>::output_size()
            }

            fn algorithm(&self) -> hash::HashAlgorithm {
                $algo
            }
        }

        struct $context($ty);

        impl hash::Context for $context {
            fn fork_finish(&self) -> hash::Output {
                hash::Output::new(&self.0.clone().finalize()[..])
            }

            fn fork(&self) -> Box<dyn hash::Context> {
                Box::new($context(self.0.clone()))
            }

            fn finish(self: Box<Self>) -> hash::Output {
                hash::Output::new(&self.0.finalize()[..])
            }

            fn update(&mut self, data: &[u8]) {
                self.0.update(data);
            }
        }

        pub(super) const $name: &dyn hash::Hash = &$hash;
    };
}

impl_hash!(SHA256, HashSha256, ContextSha256, Sha256, hash::HashAlgorithm::SHA256);
impl_hash!(SHA384, HashSha384, ContextSha384, Sha384, hash::HashAlgorithm::SHA384);

macro_rules! impl_hmac {
    ($name:ident, $hmac:ident, $key:ident, $ty:ty) => {
        struct $hmac;

        impl crypto::hmac::Hmac for $hmac {
            fn with_key(&self, key: &[u8]) -> Box<dyn crypto::hmac::Key> {
                // HMAC takes a key of any length, so this never fails; were it to, the key signs
                // nothing (an empty tag), which no peer accepts.
                Box::new($key(hmac::Hmac::<$ty>::new_from_slice(key).ok()))
            }

            fn hash_output_len(&self) -> usize {
                <$ty as OutputSizeUser>::output_size()
            }
        }

        struct $key(Option<hmac::Hmac<$ty>>);

        impl crypto::hmac::Key for $key {
            fn sign_concat(&self, first: &[u8], middle: &[&[u8]], last: &[u8]) -> crypto::hmac::Tag {
                let Some(mac) = &self.0 else { return crypto::hmac::Tag::new(&[]) };
                let mut ctx = mac.clone();
                ctx.update(first);
                for m in middle {
                    ctx.update(m);
                }
                ctx.update(last);
                crypto::hmac::Tag::new(&ctx.finalize().into_bytes()[..])
            }

            fn tag_len(&self) -> usize {
                <$ty as OutputSizeUser>::output_size()
            }
        }

        pub(super) const $name: &dyn crypto::hmac::Hmac = &$hmac;
    };
}

impl_hmac!(HMAC_SHA256, HmacSha256, HmacKeySha256, Sha256);
impl_hmac!(HMAC_SHA384, HmacSha384, HmacKeySha384, Sha384);
