//! Fuzz target `ptp`: any bytes as what a camera sends (`security.md` §4.2, §7.1). The first byte
//! picks the transfer size the bytes are cut into; the rest is the device's side of a session:
//! the device info, the session's opening, storages, handles, an object's info and its bytes in
//! chunks, then a whole listing. Each piece is also read alone as a container and as each dataset.
//! Whatever the input: refused, timed out or read within the limits; never a panic, a hang or an
//! allocation far past the input.

#![no_main]

use std::collections::VecDeque;
use std::time::Duration;

use dalica_scan::ptp::{self, Limits, Session, Transport, TransportError, container, dataset};
use libfuzzer_sys::fuzz_target;

/// A device answering every command with the next of the given transfers, then silent.
struct Script(VecDeque<Vec<u8>>);

impl Transport for Script {
    fn send(&mut self, _: &[u8], _: Duration) -> Result<(), TransportError> {
        Ok(())
    }
    fn receive(&mut self, buf: &mut [u8], _: Duration) -> Result<usize, TransportError> {
        let next = self.0.pop_front().ok_or(TransportError::TimedOut)?;
        let n = next.len().min(buf.len());
        buf[..n].copy_from_slice(&next[..n]);
        Ok(n)
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&cut, rest)) = data.split_first() else { return };
    let _ = container::parse(rest);
    let _ = dataset::DeviceInfo::parse(rest);
    let _ = dataset::StorageInfo::parse(rest);
    let _ = dataset::ObjectInfo::parse(rest);
    if let Ok(s) = std::str::from_utf8(rest) {
        let _ = dataset::parse_date(s);
    }
    let cut = usize::from(cut).max(1) * 4;
    let transfers: VecDeque<Vec<u8>> = rest.chunks(cut).map(<[u8]>::to_vec).collect();
    let limits = Limits {
        max_handles: 256,
        max_dataset: 1 << 16,
        max_thumb: 1 << 16,
        max_object: 1 << 16,
        chunk: 4096,
        timeout: Duration::from_millis(1),
        transaction: Duration::from_millis(100),
    };
    let Ok(mut s) = Session::open(Script(transfers), limits) else { return };
    if let Ok(ids) = s.storage_ids() {
        assert!(ids.len() <= 256);
    }
    if let Ok(handles) = s.object_handles(0xFFFF_FFFF) {
        assert!(handles.len() <= 256);
    }
    if let Ok(info) = s.object_info(1) {
        let size = info.size().unwrap_or(0).min(1 << 16);
        let mut got: u64 = 0;
        if let Ok(n) = s.read_object(1, size, &mut |p| {
            got += p.len() as u64;
            true
        }) {
            assert_eq!((n, got), (size, size));
        }
    }
    let _ = s.thumb(1);
    if let Ok(listing) = ptp::list(&mut s, &|| true) {
        assert!(listing.objects.len() <= 256);
    }
    s.close();
});
