//! PTP datasets (PIMA 15740 §5.3, §5.5): little-endian integers, strings (a `u8` count of UTF-16
//! units including the closing null, so at most 255), arrays (a `u32` count, then the elements),
//! and the date string `YYYYMMDDThhmmss[.s][Z|±hhmm]`; the device, storage and object datasets
//! read from them and written back (the in-memory responder answers with the writers).
//!
//! Every count is checked against the bytes left and a limit before anything is allocated.

use dalica_core::dates::{Date, DateTime};

use super::PtpError;

/// Most elements of a `u16` array in the device info (operations, events, formats …).
pub const MAX_CODES: u32 = 4096;

/// Most UTF-16 units of a string, the closing null included.
pub const MAX_STRING: usize = 255;

/// A bounded reader over a dataset's bytes.
pub struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    /// Bytes not read yet.
    pub fn left(&self) -> usize {
        self.bytes.len().saturating_sub(self.at)
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], PtpError> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.bytes.len());
        let s = end.and_then(|e| self.bytes.get(self.at..e));
        match (s, end) {
            (Some(s), Some(e)) => {
                self.at = e;
                Ok(s)
            }
            _ => Err(PtpError::Malformed(format!("the dataset ends inside {what} ({} bytes left, {n} needed)", self.left()))),
        }
    }

    pub fn u8(&mut self, what: &str) -> Result<u8, PtpError> {
        let b = self.take(1, what)?;
        b.first().copied().ok_or_else(|| PtpError::Malformed(what.to_string()))
    }

    pub fn u16(&mut self, what: &str) -> Result<u16, PtpError> {
        let b = self.take(2, what)?;
        let a: [u8; 2] = b.try_into().map_err(|_| PtpError::Malformed(what.to_string()))?;
        Ok(u16::from_le_bytes(a))
    }

    pub fn u32(&mut self, what: &str) -> Result<u32, PtpError> {
        let b = self.take(4, what)?;
        let a: [u8; 4] = b.try_into().map_err(|_| PtpError::Malformed(what.to_string()))?;
        Ok(u32::from_le_bytes(a))
    }

    pub fn u64(&mut self, what: &str) -> Result<u64, PtpError> {
        let b = self.take(8, what)?;
        let a: [u8; 8] = b.try_into().map_err(|_| PtpError::Malformed(what.to_string()))?;
        Ok(u64::from_le_bytes(a))
    }

    /// A PTP string: at most 255 UTF-16 units; cut at the first null; unpaired surrogates become
    /// U+FFFD.
    pub fn string(&mut self, what: &str) -> Result<String, PtpError> {
        let n = usize::from(self.u8(what)?);
        let b = self.take(n.saturating_mul(2), what)?;
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&u| u != 0).collect();
        Ok(String::from_utf16_lossy(&units))
    }

    /// A string, or empty when the dataset already ended (trailing fields some devices leave out).
    pub fn string_or_empty(&mut self, what: &str) -> Result<String, PtpError> {
        if self.left() == 0 { Ok(String::new()) } else { self.string(what) }
    }

    /// An array of `u16`s of at most `max` elements.
    pub fn array_u16(&mut self, max: u32, what: &str) -> Result<Vec<u16>, PtpError> {
        let n = self.count(max, 2, what)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.u16(what)?);
        }
        Ok(out)
    }

    /// An array of `u32`s of at most `max` elements.
    pub fn array_u32(&mut self, max: u32, what: &str) -> Result<Vec<u32>, PtpError> {
        let n = self.count(max, 4, what)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.u32(what)?);
        }
        Ok(out)
    }

    /// An array's count: at most `max`, and its elements must fit in the bytes left.
    fn count(&mut self, max: u32, size: usize, what: &str) -> Result<usize, PtpError> {
        let n = self.u32(what)?;
        if n > max {
            return Err(PtpError::TooLarge(format!("{what}: {n} elements (at most {max})")));
        }
        let n = usize::try_from(n).map_err(|_| PtpError::TooLarge(what.to_string()))?;
        if n.checked_mul(size).is_none_or(|bytes| bytes > self.left()) {
            return Err(PtpError::Malformed(format!("{what}: {n} elements in {} bytes", self.left())));
        }
        Ok(n)
    }
}

/// A dataset writer (the responder's side, and round trips in tests).
#[derive(Default)]
pub struct Writer {
    pub bytes: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.bytes.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }
    /// A string, cut to 254 units so the closing null fits.
    pub fn string(&mut self, s: &str) -> &mut Self {
        let mut units: Vec<u16> = s.encode_utf16().take(MAX_STRING - 1).collect();
        if units.is_empty() {
            return self.u8(0);
        }
        units.push(0);
        self.u8(u8::try_from(units.len()).unwrap_or(u8::MAX));
        for u in units {
            self.u16(u);
        }
        self
    }
    pub fn array_u16(&mut self, v: &[u16]) -> &mut Self {
        self.u32(u32::try_from(v.len()).unwrap_or(u32::MAX));
        for &x in v {
            self.u16(x);
        }
        self
    }
    pub fn array_u32(&mut self, v: &[u32]) -> &mut Self {
        self.u32(u32::try_from(v.len()).unwrap_or(u32::MAX));
        for &x in v {
            self.u32(x);
        }
        self
    }
}

/// The device dataset (`GetDeviceInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    pub standard_version: u16,
    pub vendor_extension_id: u32,
    pub vendor_extension_version: u16,
    /// For an MTP device, starts with `microsoft.com:`.
    pub vendor_extension: String,
    pub functional_mode: u16,
    pub operations: Vec<u16>,
    pub events: Vec<u16>,
    pub properties: Vec<u16>,
    pub capture_formats: Vec<u16>,
    pub image_formats: Vec<u16>,
    pub manufacturer: String,
    pub model: String,
    pub device_version: String,
    pub serial: String,
}

impl DeviceInfo {
    pub fn parse(b: &[u8]) -> Result<DeviceInfo, PtpError> {
        let mut r = Reader::new(b);
        Ok(DeviceInfo {
            standard_version: r.u16("the standard version")?,
            vendor_extension_id: r.u32("the vendor extension")?,
            vendor_extension_version: r.u16("the vendor extension")?,
            vendor_extension: r.string("the vendor extension")?,
            functional_mode: r.u16("the functional mode")?,
            operations: r.array_u16(MAX_CODES, "the operations")?,
            events: r.array_u16(MAX_CODES, "the events")?,
            properties: r.array_u16(MAX_CODES, "the properties")?,
            capture_formats: r.array_u16(MAX_CODES, "the capture formats")?,
            image_formats: r.array_u16(MAX_CODES, "the image formats")?,
            manufacturer: r.string("the manufacturer")?,
            model: r.string("the model")?,
            device_version: r.string_or_empty("the device version")?,
            serial: r.string_or_empty("the serial number")?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u16(self.standard_version)
            .u32(self.vendor_extension_id)
            .u16(self.vendor_extension_version)
            .string(&self.vendor_extension)
            .u16(self.functional_mode)
            .array_u16(&self.operations)
            .array_u16(&self.events)
            .array_u16(&self.properties)
            .array_u16(&self.capture_formats)
            .array_u16(&self.image_formats)
            .string(&self.manufacturer)
            .string(&self.model)
            .string(&self.device_version)
            .string(&self.serial);
        w.bytes
    }

    /// The device lists `op` among its operations.
    pub fn supports(&self, op: u16) -> bool {
        self.operations.contains(&op)
    }
}

/// The storage dataset (`GetStorageInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageInfo {
    pub storage_type: u16,
    pub filesystem_type: u16,
    /// 0 read-write, 1 read-only without delete, 2 read-only with delete.
    pub access: u16,
    pub max_capacity: u64,
    pub free_bytes: u64,
    pub free_images: u32,
    pub description: String,
    pub label: String,
}

impl StorageInfo {
    pub fn parse(b: &[u8]) -> Result<StorageInfo, PtpError> {
        let mut r = Reader::new(b);
        Ok(StorageInfo {
            storage_type: r.u16("the storage type")?,
            filesystem_type: r.u16("the file system type")?,
            access: r.u16("the access")?,
            max_capacity: r.u64("the capacity")?,
            free_bytes: r.u64("the free space")?,
            free_images: r.u32("the free space")?,
            description: r.string("the storage description")?,
            label: r.string_or_empty("the volume label")?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u16(self.storage_type)
            .u16(self.filesystem_type)
            .u16(self.access)
            .u64(self.max_capacity)
            .u64(self.free_bytes)
            .u32(self.free_images)
            .string(&self.description)
            .string(&self.label);
        w.bytes
    }
}

/// The object dataset (`GetObjectInfo`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ObjectInfo {
    pub storage: u32,
    pub format: u16,
    pub protection: u16,
    /// The object's size; `0xFFFFFFFF` says "more than 32 bits" (MTP), refused.
    pub compressed_size: u32,
    pub thumb_format: u16,
    pub thumb_size: u32,
    pub thumb_width: u32,
    pub thumb_height: u32,
    pub width: u32,
    pub height: u32,
    pub bit_depth: u32,
    pub parent: u32,
    pub association_type: u16,
    pub association_desc: u32,
    pub sequence: u32,
    pub filename: String,
    pub capture_date: String,
    pub modification_date: String,
    pub keywords: String,
}

impl ObjectInfo {
    pub fn parse(b: &[u8]) -> Result<ObjectInfo, PtpError> {
        let mut r = Reader::new(b);
        Ok(ObjectInfo {
            storage: r.u32("the storage id")?,
            format: r.u16("the object format")?,
            protection: r.u16("the protection status")?,
            compressed_size: r.u32("the object size")?,
            thumb_format: r.u16("the thumbnail format")?,
            thumb_size: r.u32("the thumbnail size")?,
            thumb_width: r.u32("the thumbnail width")?,
            thumb_height: r.u32("the thumbnail height")?,
            width: r.u32("the image width")?,
            height: r.u32("the image height")?,
            bit_depth: r.u32("the bit depth")?,
            parent: r.u32("the parent")?,
            association_type: r.u16("the association type")?,
            association_desc: r.u32("the association")?,
            sequence: r.u32("the sequence number")?,
            filename: r.string("the file name")?,
            capture_date: r.string_or_empty("the capture date")?,
            modification_date: r.string_or_empty("the modification date")?,
            keywords: r.string_or_empty("the keywords")?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.u32(self.storage)
            .u16(self.format)
            .u16(self.protection)
            .u32(self.compressed_size)
            .u16(self.thumb_format)
            .u32(self.thumb_size)
            .u32(self.thumb_width)
            .u32(self.thumb_height)
            .u32(self.width)
            .u32(self.height)
            .u32(self.bit_depth)
            .u32(self.parent)
            .u16(self.association_type)
            .u32(self.association_desc)
            .u32(self.sequence)
            .string(&self.filename)
            .string(&self.capture_date)
            .string(&self.modification_date)
            .string(&self.keywords);
        w.bytes
    }

    /// The object's size, or `None` when the device says it does not fit 32 bits.
    pub fn size(&self) -> Option<u64> {
        (self.compressed_size != u32::MAX).then_some(u64::from(self.compressed_size))
    }
}

/// Parses a PTP date string (`YYYYMMDDThhmmss`, then optionally tenths of a second `.s`, then
/// optionally `Z` or a `±hhmm` offset) into the date and time it shows: the wall clock the device
/// recorded, as EXIF dates are kept (an offset is checked, not applied). `None` for anything else.
pub fn parse_date(s: &str) -> Option<DateTime> {
    let b = s.as_bytes();
    if b.len() < 15 || b.len() > 32 || !s.is_ascii() || b.get(8) != Some(&b'T') {
        return None;
    }
    let num = |from: usize, len: usize| -> Option<u32> {
        let part = s.get(from..from.checked_add(len)?)?;
        if part.bytes().all(|c| c.is_ascii_digit()) { part.parse().ok() } else { None }
    };
    let date = Date::new(u16::try_from(num(0, 4)?).ok()?, u8::try_from(num(4, 2)?).ok()?, u8::try_from(num(6, 2)?).ok()?).ok()?;
    let time = DateTime::new(date, u8::try_from(num(9, 2)?).ok()?, u8::try_from(num(11, 2)?).ok()?, u8::try_from(num(13, 2)?).ok()?).ok()?;
    let mut rest = s.get(15..)?;
    if let Some(r) = rest.strip_prefix('.') {
        let digits = r.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = r.get(digits..)?;
    }
    match rest.as_bytes() {
        [] | [b'Z'] => Some(time),
        [b'+' | b'-', ..] if rest.len() == 5 => {
            let (h, m) = (num(s.len() - 4, 2)?, num(s.len() - 2, 2)?);
            (h < 24 && m < 60).then_some(time)
        }
        _ => None,
    }
}
