//! The USB transport (Linux and Windows): a device's still-image (PTP) or MTP interface found
//! from its descriptors, claimed, and its bulk pipes driven through `nusb` (pure Rust over usbfs
//! on Linux and WinUSB on Windows; no `unsafe` here).
//!
//! An interface another program holds is never taken from it: no kernel driver is detached, so a
//! phone the desktop's MTP service (gvfs, kio) mounted, or a device Windows' portable-device driver
//! owns, gives [`TransportError::Busy`] (or the platform's refusal) and stays where it is. What
//! each system lets Dalica reach is recorded in the crate README.

use std::time::Duration;

use nusb::descriptors::TransferType;
use nusb::transfer::{Buffer, Bulk, Direction, In, Out, TransferError};
use nusb::{Endpoint, ErrorKind, Interface, MaybeFuture};

use super::{Transport, TransportError};

/// USB interface class "still image" (PTP: subclass 1, protocol 1).
const CLASS_STILL_IMAGE: u8 = 6;
/// Vendor-specific class, which MTP devices without the still-image class use (interface "MTP").
const CLASS_VENDOR: u8 = 0xFF;

/// A device with a PTP or MTP interface, as the USB listing describes it (nothing opened).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbCamera {
    /// The system's id for the device while it stays plugged in.
    pub id: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    /// The PTP/MTP interface number.
    pub interface: u8,
    /// The interface is MTP's (vendor class named "MTP"), not the still-image class.
    pub mtp: bool,
    info: DeviceKey,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceKey(nusb::DeviceId);

fn error_of(e: &nusb::Error) -> TransportError {
    match e.kind() {
        ErrorKind::Disconnected => TransportError::Disconnected,
        ErrorKind::Busy => TransportError::Busy(e.to_string()),
        ErrorKind::PermissionDenied => TransportError::Denied(e.to_string()),
        _ => TransportError::Other(e.to_string()),
    }
}

fn transfer_error(e: TransferError) -> TransportError {
    match e {
        TransferError::Cancelled => TransportError::TimedOut,
        TransferError::Disconnected => TransportError::Disconnected,
        e => TransportError::Other(e.to_string()),
    }
}

/// The PTP or MTP interface of a listed device: `(number, is_mtp)`.
fn ptp_interface(d: &nusb::DeviceInfo) -> Option<(u8, bool)> {
    d.interfaces().find_map(|i| {
        if i.class() == CLASS_STILL_IMAGE && i.subclass() == 1 && i.protocol() == 1 {
            Some((i.interface_number(), false))
        } else if i.class() == CLASS_VENDOR && i.interface_string().is_some_and(|s| s.trim().eq_ignore_ascii_case("MTP")) {
            Some((i.interface_number(), true))
        } else {
            None
        }
    })
}

fn clean(s: Option<&str>) -> Option<String> {
    let s: String = s?.chars().filter(|c| !c.is_control()).take(128).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// The connected devices with a PTP or MTP interface (at most 64).
pub fn usb_cameras() -> Result<Vec<UsbCamera>, TransportError> {
    let devices = nusb::list_devices().wait().map_err(|e| error_of(&e))?;
    Ok(devices
        .filter_map(|d| {
            let (interface, mtp) = ptp_interface(&d)?;
            Some(UsbCamera {
                id: format!("{:?}", d.id()),
                vendor_id: d.vendor_id(),
                product_id: d.product_id(),
                manufacturer: clean(d.manufacturer_string()),
                product: clean(d.product_string()),
                serial: clean(d.serial_number()),
                interface,
                mtp,
                info: DeviceKey(d.id()),
            })
        })
        .take(64)
        .collect())
}

/// Every USB device the system lists (a measurement for the README; at most 256).
pub fn usb_device_count() -> Result<usize, TransportError> {
    Ok(nusb::list_devices().wait().map_err(|e| error_of(&e))?.take(256).count())
}

/// A claimed PTP/MTP interface's bulk pipes.
pub struct UsbTransport {
    _interface: Interface,
    bulk_in: Endpoint<Bulk, In>,
    bulk_out: Endpoint<Bulk, Out>,
}

/// Opens `camera` and claims its interface (never detaching a driver).
pub fn open(camera: &UsbCamera) -> Result<UsbTransport, TransportError> {
    let devices = nusb::list_devices().wait().map_err(|e| error_of(&e))?;
    let info = devices.into_iter().find(|d| d.id() == camera.info.0).ok_or(TransportError::Disconnected)?;
    let device = info.open().wait().map_err(|e| error_of(&e))?;
    let interface = device.claim_interface(camera.interface).wait().map_err(|e| error_of(&e))?;
    let desc = interface.descriptor().ok_or_else(|| TransportError::Other("the interface has no descriptor".into()))?;
    let bulk = |dir: Direction| desc.endpoints().find(|e| e.transfer_type() == TransferType::Bulk && e.direction() == dir).map(|e| e.address());
    let (Some(in_addr), Some(out_addr)) = (bulk(Direction::In), bulk(Direction::Out)) else {
        return Err(TransportError::Other("the interface has no bulk pipes".into()));
    };
    let bulk_in = interface.endpoint::<Bulk, In>(in_addr).map_err(|e| error_of(&e))?;
    let bulk_out = interface.endpoint::<Bulk, Out>(out_addr).map_err(|e| error_of(&e))?;
    Ok(UsbTransport { _interface: interface, bulk_in, bulk_out })
}

impl Transport for UsbTransport {
    fn send(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), TransportError> {
        let mut buf = Buffer::new(bytes.len());
        buf.extend_from_slice(bytes);
        let done = self.bulk_out.transfer_blocking(buf, timeout);
        done.status.map_err(transfer_error)?;
        // A container ending on a packet boundary is closed by a zero-length packet.
        let mps = self.bulk_out.max_packet_size();
        if mps > 0 && !bytes.is_empty() && bytes.len().is_multiple_of(mps) {
            self.bulk_out.transfer_blocking(Buffer::new(0), timeout).status.map_err(transfer_error)?;
        }
        Ok(())
    }

    fn receive(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize, TransportError> {
        let mps = self.bulk_in.max_packet_size().max(1);
        let ask = buf.len() / mps * mps;
        if ask == 0 {
            return Err(TransportError::Other(format!("a receive buffer of {} bytes (packets of {mps})", buf.len())));
        }
        let done = self.bulk_in.transfer_blocking(Buffer::new(ask), timeout);
        done.status.map_err(transfer_error)?;
        let got = done.buffer.get(..done.actual_len.min(done.buffer.len())).unwrap_or_default();
        let n = got.len().min(buf.len());
        buf.get_mut(..n).ok_or_else(|| TransportError::Other("buffer".into()))?.copy_from_slice(got.get(..n).unwrap_or_default());
        Ok(n)
    }
}
