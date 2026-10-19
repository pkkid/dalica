//! An in-memory PTP device for tests: a camera with storages, folders and files answering the
//! operations Dalica sends, cut into USB-like transfers, with faults to plant (silence, a short
//! container length, a hostile handle count, an unplug, more bytes than asked for). Every command
//! it receives is logged.
//!
//! Tests of this crate and of the engine's camera import use it; it never touches a device.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use super::codes::{ALL, format, op, rc};
use super::container::{self, Kind};
use super::dataset::{DeviceInfo, ObjectInfo, StorageInfo, Writer};
use super::{Transport, TransportError};

/// Bytes per USB packet the responder cuts at (a high-speed bulk endpoint).
pub const PACKET: usize = 512;

/// Most bytes in one transfer the responder hands out.
const TRANSFER: usize = 32 * PACKET;

/// Faults to plant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faults {
    /// Commands answered before the device stops answering (every later receive times out).
    pub silent_after: Option<usize>,
    /// The data container answering this operation says it is 6 bytes long.
    pub short_length: Option<u16>,
    /// `GetObjectHandles` says this many handles (then lists the real ones).
    pub handle_count: Option<u32>,
    /// `GetPartialObject` sends this many bytes more than asked for.
    pub oversend: u32,
    /// The device leaves `GetPartialObject` out of its operations.
    pub no_partial: bool,
    /// The device is unplugged: every transfer fails with `Disconnected`.
    pub unplugged: bool,
}

/// A file or folder on the in-memory device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryObject {
    pub handle: u32,
    pub info: ObjectInfo,
    pub data: Vec<u8>,
    pub thumb: Vec<u8>,
}

/// One command the responder received.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub code: u16,
    pub transaction: u32,
    pub params: Vec<u32>,
}

/// The in-memory device.
#[derive(Clone, Debug, Default)]
pub struct MemoryCamera {
    pub info: DeviceInfo,
    pub storages: Vec<(u32, StorageInfo)>,
    pub objects: Vec<MemoryObject>,
    pub faults: Faults,
    /// Every command received, in order.
    pub log: Vec<Command>,
    open: bool,
    next_handle: u32,
}

impl MemoryCamera {
    /// A PTP camera with no storage yet.
    pub fn new(manufacturer: &str, model: &str) -> MemoryCamera {
        let info = DeviceInfo {
            standard_version: 100,
            manufacturer: manufacturer.into(),
            model: model.into(),
            device_version: "1.0".into(),
            serial: "0123456789".into(),
            operations: vec![
                op::GET_DEVICE_INFO,
                op::OPEN_SESSION,
                op::CLOSE_SESSION,
                op::GET_STORAGE_IDS,
                op::GET_STORAGE_INFO,
                op::GET_OBJECT_HANDLES,
                op::GET_OBJECT_INFO,
                op::GET_OBJECT,
                op::GET_THUMB,
                op::DELETE_OBJECT,
                op::GET_PARTIAL_OBJECT,
            ],
            image_formats: vec![format::EXIF_JPEG, format::PNG],
            ..DeviceInfo::default()
        };
        MemoryCamera { info, next_handle: 1, ..MemoryCamera::default() }
    }

    /// Adds a storage labelled `label`; its id.
    pub fn add_storage(&mut self, label: &str, capacity: u64) -> u32 {
        let n = u32::try_from(self.storages.len()).unwrap_or(0xFFFF).saturating_add(1);
        let id = (n << 16) | 1;
        let info = StorageInfo {
            storage_type: 4,    // removable RAM (a card)
            filesystem_type: 3, // DCF
            access: 0,
            max_capacity: capacity,
            free_bytes: capacity / 2,
            free_images: 1000,
            description: format!("Card {n}"),
            label: label.into(),
        };
        self.storages.push((id, info));
        id
    }

    fn add(&mut self, info: ObjectInfo, data: Vec<u8>, thumb: Vec<u8>) -> u32 {
        let handle = self.next_handle;
        self.next_handle = self.next_handle.saturating_add(1);
        self.objects.push(MemoryObject { handle, info, data, thumb });
        handle
    }

    /// Adds a folder; its handle.
    pub fn add_folder(&mut self, storage: u32, parent: u32, name: &str) -> u32 {
        let info = ObjectInfo { storage, format: format::ASSOCIATION, parent, association_type: 1, filename: name.into(), ..ObjectInfo::default() };
        self.add(info, Vec::new(), Vec::new())
    }

    /// Adds a file with its bytes and capture date (a PTP date string); the format from the
    /// name's extension (JPEG, PNG, else undefined). Its handle.
    pub fn add_file(&mut self, storage: u32, parent: u32, name: &str, date: &str, data: Vec<u8>) -> u32 {
        let lower = name.to_ascii_lowercase();
        let format = if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            format::EXIF_JPEG
        } else if lower.ends_with(".png") {
            format::PNG
        } else {
            format::UNDEFINED
        };
        let info = ObjectInfo {
            storage,
            format,
            compressed_size: u32::try_from(data.len()).unwrap_or(u32::MAX),
            parent,
            filename: name.into(),
            capture_date: date.into(),
            modification_date: date.into(),
            ..ObjectInfo::default()
        };
        self.add(info, data, Vec::new())
    }

    /// The object `handle`, to change before or during a session.
    pub fn object_mut(&mut self, handle: u32) -> Option<&mut MemoryObject> {
        self.objects.iter_mut().find(|o| o.handle == handle)
    }

    /// A transport to this device, and the handle tests keep to change it and read its log.
    pub fn connect(self) -> (MemoryTransport, Arc<Mutex<MemoryCamera>>) {
        let camera = Arc::new(Mutex::new(self));
        (MemoryTransport { camera: Arc::clone(&camera), out: VecDeque::new(), answered: 0 }, camera)
    }
}

/// Locks the shared camera, a poisoned lock taken as it is.
pub fn lock(camera: &Mutex<MemoryCamera>) -> MutexGuard<'_, MemoryCamera> {
    camera.lock().unwrap_or_else(|e| e.into_inner())
}

/// The transport to a [`MemoryCamera`].
pub struct MemoryTransport {
    camera: Arc<Mutex<MemoryCamera>>,
    /// Transfers waiting to be received.
    out: VecDeque<Vec<u8>>,
    answered: usize,
}

impl MemoryTransport {
    /// Queues a container as the device would send it: transfers of at most 16 KiB, and a
    /// zero-length one after a container that ends on a packet boundary.
    fn queue(&mut self, bytes: Vec<u8>) {
        let ends_full = bytes.len().is_multiple_of(PACKET);
        for chunk in bytes.chunks(TRANSFER) {
            self.out.push_back(chunk.to_vec());
        }
        if ends_full {
            self.out.push_back(Vec::new());
        }
    }

    fn respond(&mut self, code: u16, tid: u32, params: &[u32]) {
        if let Ok(b) = container::with_params(Kind::Response, code, tid, params) {
            self.queue(b);
        }
    }

    fn data(&mut self, camera: &MemoryCamera, op_code: u16, tid: u32, payload: &[u8]) {
        if let Ok(mut b) = container::encode(Kind::Data, op_code, tid, payload) {
            if camera.faults.short_length == Some(op_code) {
                b.splice(0..4, 6u32.to_le_bytes());
            }
            self.queue(b);
        }
    }

    fn handle_command(&mut self, cmd: &Command) {
        let camera_arc = Arc::clone(&self.camera);
        let mut cam = lock(&camera_arc);
        cam.log.push(cmd.clone());
        let p = |i: usize| cmd.params.get(i).copied().unwrap_or(0);
        let tid = cmd.transaction;
        let session_free = matches!(cmd.code, op::GET_DEVICE_INFO | op::OPEN_SESSION);
        if !cam.open && !session_free {
            return self.respond(rc::SESSION_NOT_OPEN, tid, &[]);
        }
        match cmd.code {
            op::GET_DEVICE_INFO => {
                let mut info = cam.info.clone();
                if cam.faults.no_partial {
                    info.operations.retain(|&o| o != op::GET_PARTIAL_OBJECT);
                }
                self.data(&cam, cmd.code, tid, &info.encode());
                self.respond(rc::OK, tid, &[]);
            }
            op::OPEN_SESSION if cam.open => self.respond(rc::SESSION_ALREADY_OPEN, tid, &[]),
            op::OPEN_SESSION => {
                cam.open = true;
                self.respond(rc::OK, tid, &[]);
            }
            op::CLOSE_SESSION => {
                cam.open = false;
                self.respond(rc::OK, tid, &[]);
            }
            op::GET_STORAGE_IDS => {
                let ids: Vec<u32> = cam.storages.iter().map(|(id, _)| *id).collect();
                let mut w = Writer::default();
                w.array_u32(&ids);
                self.data(&cam, cmd.code, tid, &w.bytes);
                self.respond(rc::OK, tid, &[]);
            }
            op::GET_STORAGE_INFO => match cam.storages.iter().find(|(id, _)| *id == p(0)) {
                Some((_, info)) => {
                    let bytes = info.encode();
                    self.data(&cam, cmd.code, tid, &bytes);
                    self.respond(rc::OK, tid, &[]);
                }
                None => self.respond(rc::INVALID_STORAGE_ID, tid, &[]),
            },
            op::GET_OBJECT_HANDLES => {
                let storage = p(0);
                let handles: Vec<u32> = cam.objects.iter().filter(|o| storage == ALL || o.info.storage == storage).map(|o| o.handle).collect();
                let mut w = Writer::default();
                w.u32(cam.faults.handle_count.unwrap_or(u32::try_from(handles.len()).unwrap_or(u32::MAX)));
                for h in handles {
                    w.u32(h);
                }
                self.data(&cam, cmd.code, tid, &w.bytes);
                self.respond(rc::OK, tid, &[]);
            }
            op::GET_OBJECT_INFO => match cam.objects.iter().find(|o| o.handle == p(0)) {
                Some(o) => {
                    let bytes = o.info.encode();
                    self.data(&cam, cmd.code, tid, &bytes);
                    self.respond(rc::OK, tid, &[]);
                }
                None => self.respond(rc::INVALID_OBJECT_HANDLE, tid, &[]),
            },
            op::GET_OBJECT => match cam.objects.iter().find(|o| o.handle == p(0)).map(|o| o.data.clone()) {
                Some(bytes) => {
                    self.data(&cam, cmd.code, tid, &bytes);
                    self.respond(rc::OK, tid, &[]);
                }
                None => self.respond(rc::INVALID_OBJECT_HANDLE, tid, &[]),
            },
            op::GET_PARTIAL_OBJECT if cam.faults.no_partial => self.respond(rc::OPERATION_NOT_SUPPORTED, tid, &[]),
            op::GET_PARTIAL_OBJECT => match cam.objects.iter().find(|o| o.handle == p(0)).map(|o| o.data.clone()) {
                Some(bytes) => {
                    let start = (p(1) as usize).min(bytes.len());
                    let end = start.saturating_add(p(2) as usize).saturating_add(cam.faults.oversend as usize).min(bytes.len());
                    let part = bytes.get(start..end).unwrap_or_default().to_vec();
                    let sent = u32::try_from(part.len()).unwrap_or(u32::MAX);
                    self.data(&cam, cmd.code, tid, &part);
                    self.respond(rc::OK, tid, &[sent]);
                }
                None => self.respond(rc::INVALID_OBJECT_HANDLE, tid, &[]),
            },
            op::GET_THUMB => match cam.objects.iter().find(|o| o.handle == p(0)).map(|o| o.thumb.clone()) {
                Some(t) if !t.is_empty() => {
                    self.data(&cam, cmd.code, tid, &t);
                    self.respond(rc::OK, tid, &[]);
                }
                Some(_) => self.respond(rc::NO_THUMBNAIL_PRESENT, tid, &[]),
                None => self.respond(rc::INVALID_OBJECT_HANDLE, tid, &[]),
            },
            op::DELETE_OBJECT => {
                let before = cam.objects.len();
                cam.objects.retain(|o| o.handle != p(0));
                let code = if cam.objects.len() < before { rc::OK } else { rc::INVALID_OBJECT_HANDLE };
                self.respond(code, tid, &[]);
            }
            _ => self.respond(rc::OPERATION_NOT_SUPPORTED, tid, &[]),
        }
    }
}

impl Transport for MemoryTransport {
    fn send(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), TransportError> {
        let (unplugged, silent) = {
            let cam = lock(&self.camera);
            (cam.faults.unplugged, cam.faults.silent_after.is_some_and(|n| self.answered >= n))
        };
        if unplugged {
            return Err(TransportError::Disconnected);
        }
        let Ok(c) = container::parse(bytes) else {
            return Err(TransportError::Other("the responder got an unreadable container".into()));
        };
        if c.header.kind != Kind::Command {
            return Err(TransportError::Other("the responder takes only commands".into()));
        }
        if silent {
            // A silent device still takes the command, then never answers.
            let _ = timeout;
            self.out.clear();
            return Ok(());
        }
        self.answered += 1;
        let params = c.params().unwrap_or_default();
        self.handle_command(&Command { code: c.header.code, transaction: c.header.transaction, params });
        Ok(())
    }

    fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        if lock(&self.camera).faults.unplugged {
            return Err(TransportError::Disconnected);
        }
        let Some(mut next) = self.out.pop_front() else {
            std::thread::sleep(timeout);
            return Err(TransportError::TimedOut);
        };
        if next.len() > buf.len() {
            let rest = next.split_off(buf.len());
            self.out.push_front(rest);
        }
        let n = next.len();
        buf.get_mut(..n).ok_or_else(|| TransportError::Other("buffer".into()))?.copy_from_slice(&next);
        Ok(n)
    }
}
