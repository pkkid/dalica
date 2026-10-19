//! PTP containers on the USB bulk pipes (Still Image Capture Device Definition §7): a 12-byte
//! header (`u32` length of the whole container, `u16` type, `u16` code, `u32` transaction id), all
//! little-endian, then the payload: up to five `u32` parameters for a command or a response, up to
//! three for an event, the dataset or object bytes for data.

use super::PtpError;

/// Header bytes.
pub const HEADER: usize = 12;

/// Most parameters of a command or response.
pub const MAX_PARAMS: usize = 5;

/// Most parameters of an event.
pub const MAX_EVENT_PARAMS: usize = 3;

/// The length field of a data container whose payload does not fit 32 bits (MTP); refused here.
pub const UNKNOWN_LENGTH: u32 = 0xFFFF_FFFF;

/// The kind of a container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Command,
    Data,
    Response,
    Event,
}

impl Kind {
    fn code(self) -> u16 {
        match self {
            Kind::Command => 1,
            Kind::Data => 2,
            Kind::Response => 3,
            Kind::Event => 4,
        }
    }

    fn of(code: u16) -> Option<Kind> {
        match code {
            1 => Some(Kind::Command),
            2 => Some(Kind::Data),
            3 => Some(Kind::Response),
            4 => Some(Kind::Event),
            _ => None,
        }
    }
}

/// A container's header, its length checked to be at least [`HEADER`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    /// The whole container's length, header included.
    pub length: u32,
    pub kind: Kind,
    pub code: u16,
    pub transaction: u32,
}

impl Header {
    /// Payload bytes after the header.
    pub fn payload_len(&self) -> u32 {
        self.length.saturating_sub(HEADER as u32)
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// Reads a header from the first [`HEADER`] bytes of `b`: a length under 12 or an unknown type
/// is refused.
pub fn parse_header(b: &[u8]) -> Result<Header, PtpError> {
    let short = || PtpError::Malformed(format!("a container header of {} bytes", b.len()));
    let length = u32_at(b, 0).ok_or_else(short)?;
    let kind_code = u16_at(b, 4).ok_or_else(short)?;
    let code = u16_at(b, 6).ok_or_else(short)?;
    let transaction = u32_at(b, 8).ok_or_else(short)?;
    if (length as usize) < HEADER {
        return Err(PtpError::Malformed(format!("a container length of {length} (at least {HEADER})")));
    }
    let kind = Kind::of(kind_code).ok_or_else(|| PtpError::Malformed(format!("a container of type {kind_code}")))?;
    Ok(Header { length, kind, code, transaction })
}

/// A whole container held in memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub header: Header,
    pub payload: Vec<u8>,
}

impl Container {
    /// The parameters of a command, response or event payload (a whole number of `u32`s, at most
    /// five; three for an event).
    pub fn params(&self) -> Result<Vec<u32>, PtpError> {
        let most = if self.header.kind == Kind::Event { MAX_EVENT_PARAMS } else { MAX_PARAMS };
        params(&self.payload, most)
    }
}

/// The parameters in `payload`: a whole number of `u32`s, at most `most`.
pub fn params(payload: &[u8], most: usize) -> Result<Vec<u32>, PtpError> {
    if !payload.len().is_multiple_of(4) || payload.len() / 4 > most {
        return Err(PtpError::Malformed(format!("{} bytes of parameters (at most {most} of 4 bytes)", payload.len())));
    }
    Ok(payload.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c)).collect())
}

/// Reads one whole container that is exactly `b` (its length field must equal `b.len()`).
pub fn parse(b: &[u8]) -> Result<Container, PtpError> {
    let header = parse_header(b)?;
    if header.length as usize != b.len() {
        return Err(PtpError::Malformed(format!("a container saying {} bytes in {}", header.length, b.len())));
    }
    let payload = b.get(HEADER..).unwrap_or_default().to_vec();
    Ok(Container { header, payload })
}

/// Encodes a container; refused when the payload does not fit the 32-bit length.
pub fn encode(kind: Kind, code: u16, transaction: u32, payload: &[u8]) -> Result<Vec<u8>, PtpError> {
    let length = payload
        .len()
        .checked_add(HEADER)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|&n| n != UNKNOWN_LENGTH)
        .ok_or_else(|| PtpError::TooLarge(format!("a payload of {} bytes", payload.len())))?;
    let mut out = Vec::with_capacity(length as usize);
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&kind.code().to_le_bytes());
    out.extend_from_slice(&code.to_le_bytes());
    out.extend_from_slice(&transaction.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(out)
}

/// Encodes a command, response or event with its parameters (at most five).
pub fn with_params(kind: Kind, code: u16, transaction: u32, params: &[u32]) -> Result<Vec<u8>, PtpError> {
    if params.len() > MAX_PARAMS {
        return Err(PtpError::Refused(format!("{} parameters (at most {MAX_PARAMS})", params.len())));
    }
    let payload: Vec<u8> = params.iter().flat_map(|p| p.to_le_bytes()).collect();
    encode(kind, code, transaction, &payload)
}
