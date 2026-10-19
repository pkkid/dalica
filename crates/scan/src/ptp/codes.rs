//! The PTP and MTP codes Dalica uses (PIMA 15740 §10–§12 and annex; MTP 1.1 annex A).

/// Operation codes.
pub mod op {
    pub const GET_DEVICE_INFO: u16 = 0x1001;
    pub const OPEN_SESSION: u16 = 0x1002;
    pub const CLOSE_SESSION: u16 = 0x1003;
    pub const GET_STORAGE_IDS: u16 = 0x1004;
    pub const GET_STORAGE_INFO: u16 = 0x1005;
    pub const GET_OBJECT_HANDLES: u16 = 0x1007;
    pub const GET_OBJECT_INFO: u16 = 0x1008;
    pub const GET_OBJECT: u16 = 0x1009;
    pub const GET_THUMB: u16 = 0x100A;
    pub const DELETE_OBJECT: u16 = 0x100B;
    pub const GET_PARTIAL_OBJECT: u16 = 0x101B;
}

/// Response codes.
pub mod rc {
    pub const OK: u16 = 0x2001;
    pub const GENERAL_ERROR: u16 = 0x2002;
    pub const SESSION_NOT_OPEN: u16 = 0x2003;
    pub const OPERATION_NOT_SUPPORTED: u16 = 0x2005;
    pub const INVALID_STORAGE_ID: u16 = 0x2008;
    pub const INVALID_OBJECT_HANDLE: u16 = 0x2009;
    pub const OBJECT_WRITE_PROTECTED: u16 = 0x200D;
    pub const ACCESS_DENIED: u16 = 0x200F;
    pub const NO_THUMBNAIL_PRESENT: u16 = 0x2010;
    pub const DEVICE_BUSY: u16 = 0x2019;
    pub const INVALID_PARAMETER: u16 = 0x201D;
    pub const SESSION_ALREADY_OPEN: u16 = 0x201E;
}

/// Object format codes.
pub mod format {
    pub const UNDEFINED: u16 = 0x3000;
    /// A folder.
    pub const ASSOCIATION: u16 = 0x3001;
    pub const AVI: u16 = 0x300A;
    pub const MPEG: u16 = 0x300B;
    pub const ASF: u16 = 0x300C;
    pub const QUICKTIME: u16 = 0x300D;
    /// The first and last image format codes (`0x3800` "unknown image" to `0x38FF`).
    pub const IMAGE_FIRST: u16 = 0x3800;
    pub const IMAGE_LAST: u16 = 0x38FF;
    pub const EXIF_JPEG: u16 = 0x3801;
    pub const PNG: u16 = 0x380B;
    pub const TIFF: u16 = 0x380D;
    pub const DNG: u16 = 0x3811;
    /// MTP's video formats (`0xB980` undefined video to `0xB98F`: WMV, MP4, MP2, 3GP …).
    pub const MTP_VIDEO_FIRST: u16 = 0xB980;
    pub const MTP_VIDEO_LAST: u16 = 0xB98F;
}

/// Event code: the device's state changed (sent on the bulk pipe by some devices; skipped).
pub const EVENT_DEVICE_INFO_CHANGED: u16 = 0x4008;

/// The storage id and parent handle that ask for every object (`GetObjectHandles`).
pub const ALL: u32 = 0xFFFF_FFFF;

/// A readable name of an operation code, for messages.
pub fn operation_name(code: u16) -> String {
    let name = match code {
        op::GET_DEVICE_INFO => "GetDeviceInfo",
        op::OPEN_SESSION => "OpenSession",
        op::CLOSE_SESSION => "CloseSession",
        op::GET_STORAGE_IDS => "GetStorageIDs",
        op::GET_STORAGE_INFO => "GetStorageInfo",
        op::GET_OBJECT_HANDLES => "GetObjectHandles",
        op::GET_OBJECT_INFO => "GetObjectInfo",
        op::GET_OBJECT => "GetObject",
        op::GET_THUMB => "GetThumb",
        op::DELETE_OBJECT => "DeleteObject",
        op::GET_PARTIAL_OBJECT => "GetPartialObject",
        _ => return format!("operation 0x{code:04X}"),
    };
    name.to_string()
}

/// A readable name of a response code, for messages.
pub fn response_name(code: u16) -> String {
    let name = match code {
        rc::OK => "OK",
        rc::GENERAL_ERROR => "general error",
        rc::SESSION_NOT_OPEN => "session not open",
        rc::OPERATION_NOT_SUPPORTED => "not supported",
        rc::INVALID_STORAGE_ID => "no such storage",
        rc::INVALID_OBJECT_HANDLE => "no such object",
        rc::OBJECT_WRITE_PROTECTED => "the object is protected",
        rc::ACCESS_DENIED => "access denied",
        rc::NO_THUMBNAIL_PRESENT => "no thumbnail",
        rc::DEVICE_BUSY => "the device is busy",
        rc::INVALID_PARAMETER => "invalid parameter",
        rc::SESSION_ALREADY_OPEN => "a session is already open",
        _ => return format!("response 0x{code:04X}"),
    };
    name.to_string()
}
