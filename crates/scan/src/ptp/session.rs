//! A PTP session on one device (PIMA 15740 §9–§10): transactions of a command, at most one data
//! phase from the device, and a response, each within [`Limits::transaction`].

use std::time::Instant;

use super::codes::{ALL, op, rc};
use super::container::{self, HEADER, Kind, UNKNOWN_LENGTH};
use super::dataset::{DeviceInfo, ObjectInfo, Reader, StorageInfo};
use super::transport::Link;
use super::{Limits, PtpError, Transport};

/// The session id Dalica opens (any non-zero value).
const SESSION_ID: u32 = 1;

/// Where a transaction's data phase goes: at most this many bytes, handed to the closure in
/// pieces (`false` stops).
type DataSink<'a> = (u64, &'a mut dyn FnMut(&[u8]) -> bool);

/// Most events skipped inside one transaction before the stream is called broken.
const MAX_EVENTS: u32 = 64;

/// An open session on a device. Every method sends one or more transactions; after any error
/// inside one the session is broken ([`PtpError::Broken`] from then on): open a new one.
pub struct Session<T: Transport> {
    link: Link<T>,
    limits: Limits,
    info: DeviceInfo,
    /// The next transaction id (0 is OpenSession's; ids then run 1..=0xFFFFFFFE).
    next: u32,
    broken: bool,
}

impl<T: Transport> Session<T> {
    /// Asks the device for its info and opens a session (a device saying one is already open is
    /// taken as open).
    pub fn open(transport: T, limits: Limits) -> Result<Session<T>, PtpError> {
        let mut s = Session { link: Link::new(transport, limits.timeout), limits, info: DeviceInfo::default(), next: 0, broken: false };
        let bytes = s.transact_collect(op::GET_DEVICE_INFO, &[], limits.max_dataset)?;
        s.info = DeviceInfo::parse(&bytes)?;
        s.next = 0;
        match s.transact(op::OPEN_SESSION, &[SESSION_ID], None) {
            Ok(_) | Err(PtpError::Device { code: rc::SESSION_ALREADY_OPEN, .. }) => {}
            Err(e) => return Err(e),
        }
        s.next = 1;
        s.broken = false;
        Ok(s)
    }

    /// The device's info, read when the session opened.
    pub fn device_info(&self) -> &DeviceInfo {
        &self.info
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    /// The session refuses further operations.
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// The device's storages (at most 256).
    pub fn storage_ids(&mut self) -> Result<Vec<u32>, PtpError> {
        let bytes = self.transact_collect(op::GET_STORAGE_IDS, &[], self.limits.max_dataset)?;
        Reader::new(&bytes).array_u32(256, "the storage ids")
    }

    pub fn storage_info(&mut self, storage: u32) -> Result<StorageInfo, PtpError> {
        let bytes = self.transact_collect(op::GET_STORAGE_INFO, &[storage], self.limits.max_dataset)?;
        StorageInfo::parse(&bytes)
    }

    /// The handles of every object on `storage` (`ALL` for every storage), at most
    /// [`Limits::max_handles`]; a device listing more is refused.
    pub fn object_handles(&mut self, storage: u32) -> Result<Vec<u32>, PtpError> {
        let max = self.limits.max_handles;
        let bytes_max = max.saturating_mul(4).saturating_add(4).max(self.limits.max_dataset);
        let bytes = self.transact_collect(op::GET_OBJECT_HANDLES, &[storage, 0, 0], bytes_max)?;
        let mut r = Reader::new(&bytes);
        let handles = r.array_u32(max, "the object handles")?;
        Ok(handles)
    }

    pub fn object_info(&mut self, handle: u32) -> Result<ObjectInfo, PtpError> {
        let bytes = self.transact_collect(op::GET_OBJECT_INFO, &[handle], self.limits.max_dataset)?;
        ObjectInfo::parse(&bytes)
    }

    /// The object's thumbnail (at most [`Limits::max_thumb`] bytes).
    pub fn thumb(&mut self, handle: u32) -> Result<Vec<u8>, PtpError> {
        self.transact_collect(op::GET_THUMB, &[handle], self.limits.max_thumb)
    }

    /// Reads the object `handle` of `size` bytes (its info's size) into `sink`, in pieces:
    /// `GetPartialObject` in chunks of [`Limits::chunk`] when the device has it, else one
    /// `GetObject` streamed. A device sending more than `size` (or more than a chunk asked for),
    /// or less, is refused; so is a size past [`Limits::max_object`]. `sink` returning `false`
    /// stops ([`PtpError::Stopped`]). The bytes read.
    pub fn read_object(&mut self, handle: u32, size: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<u64, PtpError> {
        check_handle(handle)?;
        if size > self.limits.max_object {
            return Err(PtpError::TooLarge(format!("an object of {size} bytes (at most {})", self.limits.max_object)));
        }
        if !self.info.supports(op::GET_PARTIAL_OBJECT) {
            return self.read_whole(handle, size, sink);
        }
        let chunk = self.limits.chunk.max(1);
        let mut done: u64 = 0;
        while done < size {
            let offset = u32::try_from(done).map_err(|_| PtpError::TooLarge(format!("an offset of {done}")))?;
            let ask = u32::try_from((size - done).min(u64::from(chunk))).unwrap_or(chunk);
            let mut got: u64 = 0;
            let mut stop = false;
            let params = self.transact(
                op::GET_PARTIAL_OBJECT,
                &[handle, offset, ask],
                Some((u64::from(ask), &mut |piece: &[u8]| {
                    got = got.saturating_add(piece.len() as u64);
                    let more = sink(piece);
                    stop |= !more;
                    more
                })),
            )?;
            if stop {
                return Err(PtpError::Stopped);
            }
            if params.first().is_some_and(|&n| u64::from(n) != got) {
                return self.fail(PtpError::Malformed(format!("a chunk of {got} bytes reported as {:?}", params.first())));
            }
            if got == 0 {
                return self.fail(PtpError::Malformed(format!("object {handle:#x} ended at {done} of {size} bytes")));
            }
            done = done.saturating_add(got);
        }
        // One byte past the end: a device that has it holds an object larger than its info said.
        if let Ok(offset) = u32::try_from(size) {
            let mut extra: u64 = 0;
            match self.transact(
                op::GET_PARTIAL_OBJECT,
                &[handle, offset, 1],
                Some((1, &mut |p: &[u8]| {
                    extra = extra.saturating_add(p.len() as u64);
                    true
                })),
            ) {
                Ok(_) if extra > 0 => {
                    return self.fail(PtpError::TooLarge(format!("object {handle:#x} is larger than the {size} bytes its info says")));
                }
                Ok(_) | Err(PtpError::Device { .. }) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }

    fn read_whole(&mut self, handle: u32, size: u64, sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<u64, PtpError> {
        let mut got: u64 = 0;
        self.transact(
            op::GET_OBJECT,
            &[handle],
            Some((size, &mut |piece: &[u8]| {
                got = got.saturating_add(piece.len() as u64);
                sink(piece)
            })),
        )?;
        if got != size {
            return self.fail(PtpError::Malformed(format!("object {handle:#x} sent {got} of the {size} bytes its info says")));
        }
        Ok(got)
    }

    /// Deletes the one object `handle` (never every object: `0xFFFFFFFF` and 0 are refused).
    pub fn delete_object(&mut self, handle: u32) -> Result<(), PtpError> {
        check_handle(handle)?;
        self.transact(op::DELETE_OBJECT, &[handle], None).map(|_| ())
    }

    /// Closes the session (errors ignored: the device may be gone).
    pub fn close(mut self) {
        if !self.broken {
            let _ = self.transact(op::CLOSE_SESSION, &[], None);
        }
    }

    /// The transport, for tests that inspect it after a session.
    pub fn into_transport(self) -> T {
        self.link.transport
    }

    fn fail<V>(&mut self, e: PtpError) -> Result<V, PtpError> {
        self.broken = true;
        Err(e)
    }

    /// A transaction whose data phase is collected (at most `max` bytes).
    fn transact_collect(&mut self, code: u16, params: &[u32], max: u32) -> Result<Vec<u8>, PtpError> {
        let mut out = Vec::new();
        self.transact(
            code,
            params,
            Some((u64::from(max), &mut |piece: &[u8]| {
                out.extend_from_slice(piece);
                true
            })),
        )?;
        Ok(out)
    }

    /// Sends one command and reads to its response; the data phase (if the device sends one, at
    /// most `data.0` bytes) goes to `data.1`. The response's parameters.
    fn transact(&mut self, code: u16, params: &[u32], data: Option<DataSink<'_>>) -> Result<Vec<u32>, PtpError> {
        if self.broken {
            return Err(PtpError::Broken);
        }
        let result = self.transact_inner(code, params, data);
        match &result {
            // A device's refusal ends the transaction cleanly; anything else may leave the pipes
            // out of step.
            Ok(_) | Err(PtpError::Device { .. }) => {}
            Err(_) => self.broken = true,
        }
        result
    }

    fn transact_inner(&mut self, code: u16, params: &[u32], mut data: Option<DataSink<'_>>) -> Result<Vec<u32>, PtpError> {
        let deadline = Instant::now() + self.limits.transaction;
        let tid = self.next;
        self.next = match self.next.wrapping_add(1) {
            0 | ALL => 1,
            n => n,
        };
        let command = container::with_params(Kind::Command, code, tid, params)?;
        self.link.send(&command, deadline)?;
        let mut had_data = false;
        let mut events = 0;
        loop {
            let head = self.link.read_exact(HEADER, deadline)?;
            let header = container::parse_header(&head)?;
            match header.kind {
                Kind::Event => {
                    events += 1;
                    let len = header.payload_len() as usize;
                    if events > MAX_EVENTS || len > container::MAX_EVENT_PARAMS * 4 {
                        return Err(PtpError::Malformed(format!("an event of {} bytes inside a transaction", header.length)));
                    }
                    self.link.read_exact(len, deadline)?;
                }
                Kind::Data => {
                    if had_data || header.code != code || header.transaction != tid {
                        return Err(PtpError::Malformed(format!(
                            "a data phase for 0x{:04X} #{} answering 0x{code:04X} #{tid}",
                            header.code, header.transaction
                        )));
                    }
                    had_data = true;
                    if header.length == UNKNOWN_LENGTH {
                        return Err(PtpError::TooLarge("a data phase longer than 4 GiB".into()));
                    }
                    let len = u64::from(header.payload_len());
                    let Some((max, sink)) = data.as_mut() else {
                        return Err(PtpError::Malformed(format!("a data phase for {}, which has none", super::codes::operation_name(code))));
                    };
                    if len > *max {
                        return Err(PtpError::TooLarge(format!("{len} bytes answering {} (at most {max})", super::codes::operation_name(code))));
                    }
                    self.link.read_into(len, deadline, &mut **sink)?;
                }
                Kind::Response => {
                    let len = header.payload_len() as usize;
                    if len > container::MAX_PARAMS * 4 {
                        return Err(PtpError::Malformed(format!("a response of {} bytes", header.length)));
                    }
                    let payload = self.link.read_exact(len, deadline)?;
                    if header.transaction != tid {
                        return Err(PtpError::Malformed(format!("the response to #{} answering #{tid}", header.transaction)));
                    }
                    let params = container::params(&payload, container::MAX_PARAMS)?;
                    if header.code != rc::OK {
                        return Err(PtpError::Device { op: code, code: header.code });
                    }
                    return Ok(params);
                }
                Kind::Command => return Err(PtpError::Malformed("a command from the device".into())),
            }
        }
    }
}

fn check_handle(handle: u32) -> Result<(), PtpError> {
    if handle == 0 || handle == ALL {
        return Err(PtpError::Refused(format!("the object handle {handle:#x}")));
    }
    Ok(())
}
