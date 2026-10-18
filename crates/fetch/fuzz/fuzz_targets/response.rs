//! Fuzz target `response`: any bytes as a server's reply (`security.md` §7.1, §8.2). The first
//! byte sets how many bytes each read of the wire hands out (1 to 256), so line and chunk
//! boundaries fall everywhere; the rest is read as the HTTPS transport reads a reply after the TLS
//! layer: the status line and headers (capped), then the body by its length, its chunks or the end
//! of the stream. Whatever the input: an error, or a reply whose status is 200–599, whose body is
//! no longer than the input and exactly its announced length; a `Location` it carries leads only
//! to an https address or is refused. Never a panic, a hang or an allocation past the input.

#![no_main]

use dalica_fetch::Url;
use dalica_fetch::http::{read_reply, redirect_target};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, reply)) = data.split_first() else { return };
    let chunk = usize::from(first) + 1;
    let Ok(r) = read_reply(reply, chunk) else { return };
    assert!((200..=599).contains(&r.status), "{}", r.status);
    assert!(r.body.len() <= reply.len());
    if let Some(n) = r.length {
        assert_eq!(r.body.len() as u64, n);
    }
    if let Some(location) = &r.location {
        let Ok(from) = Url::parse("https://models.example.org/repo/file.onnx?sig=1") else { return };
        if let Ok(to) = redirect_target(&from, location) {
            assert!(to.to_string().starts_with("https://"));
        }
    }
});
