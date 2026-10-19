//! The pipes a session talks over, and the bounded byte stream the session reads containers from.

use std::fmt;
use std::time::{Duration, Instant};

use super::PtpError;

/// Why a transfer failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportError {
    /// Nothing within the time allowed.
    TimedOut,
    /// The device was unplugged.
    Disconnected,
    /// Another program or a driver holds the device (the desktop's own MTP service, the
    /// system's portable-device driver).
    Busy(String),
    /// This user may not open the device.
    Denied(String),
    /// Anything else the USB stack said.
    Other(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportError::TimedOut => f.write_str("the device stopped answering"),
            TransportError::Disconnected => f.write_str("the device was disconnected"),
            TransportError::Busy(why) => write!(f, "another program is using the device ({why})"),
            TransportError::Denied(why) => write!(f, "no permission to open the device ({why})"),
            TransportError::Other(why) => write!(f, "USB error: {why}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// One device's bulk pipes. `receive` returns one transfer: at most `buf.len()` bytes, ending
/// early at a short packet like a USB bulk-in transfer (a zero-length one is allowed).
pub trait Transport: Send {
    /// Sends `bytes` on the bulk-out pipe within `timeout`.
    fn send(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), TransportError>;
    /// Receives one transfer into `buf` within `timeout`; the number of bytes received.
    fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError>;
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), TransportError> {
        (**self).send(bytes, timeout)
    }
    fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        (**self).receive(buf, timeout)
    }
}

/// Size of one bulk-in transfer asked for: a multiple of every USB packet size (64, 512, 1024).
pub(crate) const TRANSFER: usize = 64 * 1024;

/// Most zero-length transfers in a row before the stream is called broken.
const MAX_EMPTY: u32 = 64;

/// A byte stream over the bulk-in transfers, with a deadline per transaction.
pub(crate) struct Link<T> {
    pub(crate) transport: T,
    /// Received but not consumed yet: `pending[start..]`.
    pending: Vec<u8>,
    start: usize,
    buf: Vec<u8>,
    /// Longest wait for one transfer.
    pub(crate) timeout: Duration,
}

impl<T: Transport> Link<T> {
    pub(crate) fn new(transport: T, timeout: Duration) -> Link<T> {
        Link { transport, pending: Vec::new(), start: 0, buf: vec![0; TRANSFER], timeout }
    }

    fn wait(&self, deadline: Instant) -> Result<Duration, PtpError> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(PtpError::TimedOut);
        }
        Ok(left.min(self.timeout))
    }

    pub(crate) fn send(&mut self, bytes: &[u8], deadline: Instant) -> Result<(), PtpError> {
        let wait = self.wait(deadline)?;
        self.transport.send(bytes, wait).map_err(PtpError::from)
    }

    fn buffered(&self) -> usize {
        self.pending.len().saturating_sub(self.start)
    }

    /// Receives one more transfer into the pending bytes.
    fn fill(&mut self, deadline: Instant) -> Result<(), PtpError> {
        let mut empty = 0;
        loop {
            let wait = self.wait(deadline)?;
            let n = self.transport.receive(&mut self.buf, wait)?;
            let got = self.buf.get(..n).ok_or_else(|| PtpError::Malformed(format!("a transfer of {n} bytes into {}", self.buf.len())))?;
            if got.is_empty() {
                empty += 1;
                if empty > MAX_EMPTY {
                    return Err(PtpError::Malformed("endless empty transfers".into()));
                }
                continue;
            }
            if self.start > 0 && self.start == self.pending.len() {
                self.pending.clear();
                self.start = 0;
            } else if self.start >= TRANSFER {
                self.pending.drain(..self.start);
                self.start = 0;
            }
            self.pending.extend_from_slice(got);
            return Ok(());
        }
    }

    /// Exactly `n` bytes (the caller bounds `n`).
    pub(crate) fn read_exact(&mut self, n: usize, deadline: Instant) -> Result<Vec<u8>, PtpError> {
        let mut out = Vec::with_capacity(n.min(TRANSFER));
        self.read_into(n as u64, deadline, &mut |piece| {
            out.extend_from_slice(piece);
            true
        })?;
        Ok(out)
    }

    /// Exactly `n` bytes handed to `sink` in pieces; `sink` returning `false` stops with
    /// [`PtpError::Stopped`].
    pub(crate) fn read_into(&mut self, n: u64, deadline: Instant, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), PtpError> {
        let mut left = n;
        while left > 0 {
            if self.buffered() == 0 {
                self.fill(deadline)?;
            }
            let take = usize::try_from(left).unwrap_or(usize::MAX).min(self.buffered());
            let end = self.start.saturating_add(take);
            let piece = self.pending.get(self.start..end).ok_or_else(|| PtpError::Malformed("stream".into()))?;
            let more = sink(piece);
            self.start = end;
            left = left.saturating_sub(take as u64);
            if !more {
                return Err(PtpError::Stopped);
            }
        }
        Ok(())
    }
}
