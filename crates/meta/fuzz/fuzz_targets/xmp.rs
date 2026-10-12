//! Fuzz target `xmp`: the XMP entry points with a packet as given, and the JPEG walk with the input split into a standard packet and extended-XMP chunks (reassembly by GUID, bounded).

#![no_main]

use dalica_meta::{Limits, container, listing, read, xmp};
use libfuzzer_sys::fuzz_target;

const GUID: &[u8; 32] = b"0123456789ABCDEF0123456789ABCDEF";

fn segment(out: &mut Vec<u8>, payload: &[u8]) {
    if payload.len() + 2 <= 0xffff {
        out.extend([0xff, 0xe1]);
        out.extend(((payload.len() + 2) as u16).to_be_bytes());
        out.extend(payload);
    }
}

fuzz_target!(|data: &[u8]| {
    let l = Limits { max_xmp: 1 << 20, ..Limits::default() };
    let _ = xmp::read(data, None, &l);
    let _ = xmp::rows(data, Some(data), &l);
    // First byte: how many chunks; the rest split between the packet and the chunks, each chunk
    // taking its declared length and offset from its first eight bytes.
    let Some((&n, rest)) = data.split_first() else { return };
    let parts: Vec<&[u8]> = rest.chunks(rest.len() / (usize::from(n % 8) + 1) + 1).collect();
    let mut file = vec![0xff, 0xd8];
    let main = [&b"http://ns.adobe.com/xap/1.0/\0<x xmpNote:HasExtendedXMP=\""[..], GUID, b"\"/>", parts.first().copied().unwrap_or_default()].concat();
    segment(&mut file, &main);
    for part in parts.iter().skip(1) {
        let (head, body) = part.split_at(part.len().min(8));
        let mut full = [0u8; 4];
        let mut offset = [0u8; 4];
        for (i, b) in head.iter().enumerate() {
            if i < 4 { full[i] = *b } else { offset[i - 4] = *b }
        }
        let chunk = [&b"http://ns.adobe.com/xmp/extension/\0"[..], GUID, &full, &offset, body].concat();
        segment(&mut file, &chunk);
    }
    file.extend([0xff, 0xda, 0, 2, 0xff, 0xd9]);
    if let Ok(scan) = container::scan(&file, &l) {
        let _ = xmp::read(scan.xmp.as_deref().unwrap_or_default(), scan.extended_xmp.as_deref(), &l);
    }
    let _ = read(&file, &l);
    let _ = listing(&file, &l);
});
