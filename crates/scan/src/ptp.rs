// Provenance: written for Dalica from the public specifications only: PIMA 15740:2000 / ISO 15740
// (Picture Transfer Protocol: containers, operations, datasets, response codes, the date string),
// the USB-IF "Still Image Capture Device Definition" 1.0 (the bulk-only container framing) and the
// USB-IF "Media Transfer Protocol" 1.1 (the MTP interface, object formats, sizes past 4 GiB). No
// other implementation's code was read.

//! Cameras and phones over PTP and MTP (`organizing.md` §3 item 1, `security.md` §5.9): our own
//! reader of the Picture Transfer Protocol and its MTP extension.
//!
//! - [`container`]: the four kinds of container (command, data, response, event), their lengths
//!   checked before anything is read past the header.
//! - [`dataset`]: the bounded reader of PTP datasets (integers, strings of at most 255 UTF-16
//!   units, arrays of at most [`Limits::max_handles`] elements, dates) and the device, storage and
//!   object datasets, with their writers (the in-memory responder answers with them).
//! - [`Transport`]: one bulk-out and one bulk-in pipe, each transfer with a timeout. The USB one
//!   (`usb`, Linux and Windows, through `nusb`) and the in-memory [`responder`] implement it.
//! - [`Session`]: a session on one device: device info, storages, object handles and infos,
//!   thumbnails, objects read in bounded chunks (`GetPartialObject`, else a streamed `GetObject`),
//!   `DeleteObject` for one handle, and closing. Every transaction has a deadline: a device that
//!   stops answering is timed out, never waited for.
//! - [`camera`]: a whole device listed (its storages and every file on them, names through
//!   `core::names::sanitize`, dates parsed from their strings).
//!
//! Everything the device sends is hostile input: lengths and counts are checked against what was
//! received and against [`Limits`] before any allocation, arithmetic is checked, and an object is
//! never read past the size its info gave (at most 4 GiB). After any error inside a transaction the
//! session is broken (the pipes may be out of step) and refuses further operations: open a new
//! one. Nothing here panics.
//!
//! Everything here talks to a device: run it on a worker, never the UI thread.

pub mod camera;
pub mod codes;
pub mod container;
pub mod dataset;
pub mod responder;
mod session;
mod transport;
#[cfg(all(feature = "usb", any(target_os = "linux", target_os = "windows")))]
pub mod usb;

use std::fmt;
use std::time::Duration;

pub use camera::{Listing, Object, ObjectKind, Storage, list};
pub use dataset::{DeviceInfo, ObjectInfo, StorageInfo, parse_date};
pub use session::Session;
pub use transport::{Transport, TransportError};

/// The most an object may be: 4 GiB (`ObjectCompressedSize` is 32 bits; a device saying
/// `0xFFFFFFFF` means "larger", which is refused).
pub const MAX_OBJECT: u64 = 4 << 30;

/// The most handles a device may list (each storage's and all storages' together).
pub const MAX_HANDLES: u32 = 100_000;

/// The bounds a session reads within.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most handles in one listing (and over all storages): [`MAX_HANDLES`].
    pub max_handles: u32,
    /// Largest object read: [`MAX_OBJECT`].
    pub max_object: u64,
    /// Largest dataset (device, storage or object info): 1 MiB.
    pub max_dataset: u32,
    /// Largest thumbnail: 4 MiB.
    pub max_thumb: u32,
    /// Bytes asked for in one `GetPartialObject`: 1 MiB.
    pub chunk: u32,
    /// Longest wait for one transfer: 5 s.
    pub timeout: Duration,
    /// Longest one transaction may take, start to end (a device trickling bytes): 60 s.
    pub transaction: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_handles: MAX_HANDLES,
            max_object: MAX_OBJECT,
            max_dataset: 1 << 20,
            max_thumb: 4 << 20,
            chunk: 1 << 20,
            timeout: Duration::from_secs(5),
            transaction: Duration::from_secs(60),
        }
    }
}

/// Why a PTP operation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PtpError {
    /// The transport failed: the device is gone, held by another program, or not ours to open.
    Transport(TransportError),
    /// The device did not answer in time.
    TimedOut,
    /// What the device sent breaks the protocol.
    Malformed(String),
    /// A length, count or size past a limit, or an object larger than its info said.
    TooLarge(String),
    /// The device answered an operation with a response code other than OK.
    Device {
        /// The operation code.
        op: u16,
        /// The response code.
        code: u16,
    },
    /// The session was broken by an earlier error (or closed): open a new one.
    Broken,
    /// The caller's sink asked to stop.
    Stopped,
    /// The caller asked for something the reader refuses (a handle of 0 or `0xFFFFFFFF`).
    Refused(String),
}

impl fmt::Display for PtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PtpError::Transport(e) => write!(f, "{e}"),
            PtpError::TimedOut => f.write_str("the device stopped answering"),
            PtpError::Malformed(why) => write!(f, "the device sent something unreadable: {why}"),
            PtpError::TooLarge(why) => write!(f, "past a limit: {why}"),
            PtpError::Device { op, code } => {
                write!(f, "the device refused {} ({})", codes::operation_name(*op), codes::response_name(*code))
            }
            PtpError::Broken => f.write_str("the connection to the device was lost; open it again"),
            PtpError::Stopped => f.write_str("stopped"),
            PtpError::Refused(why) => write!(f, "refused: {why}"),
        }
    }
}

impl std::error::Error for PtpError {}

impl From<TransportError> for PtpError {
    fn from(e: TransportError) -> Self {
        match e {
            TransportError::TimedOut => PtpError::TimedOut,
            e => PtpError::Transport(e),
        }
    }
}
