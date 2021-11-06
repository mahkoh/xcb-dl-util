#![allow(non_camel_case_types)]

use crate::xcb_box::XcbBox;
use bstr::ByteSlice;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::os::raw::c_int;
use std::{ptr, slice};
use thiserror::Error;
use xcb_dl::ffi::*;
use xcb_dl::Xcb;

#[derive(Clone, Debug)]
pub struct XcbError {
    pub error_code: u8,
    pub sequence: u32,
    pub major: u8,
    pub minor: u16,
    pub ty: XcbErrorType,
}

impl Display for XcbError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.ty, f)
    }
}

impl Error for XcbError {}

impl From<XcbErrorType> for XcbError {
    fn from(e: XcbErrorType) -> Self {
        Self {
            error_code: 0,
            sequence: 0,
            major: 0,
            minor: 0,
            ty: e,
        }
    }
}

#[derive(Clone, Error, Debug)]
#[non_exhaustive]
pub enum XcbConnectionError {
    #[error("An unknown error occurred: {0}")]
    Unknown(c_int),
    #[error("An IO error occurred")]
    Io,
    #[error("The user tried to send a request with an unsupported extension")]
    UnsupportedExtension,
    #[error("Out of memory")]
    OutOfMemory,
    #[error("The user tried to send a request that is too large for the X server")]
    MessageLength,
    #[error("libxcb was unable to parse the DISPLAY string")]
    DisplayString,
    #[error("The requested screen is not available")]
    InvalidScreen,
    #[error("An error occurred while handling file descriptors passed to and from the X server")]
    FileDescriptors,
}

impl From<c_int> for XcbConnectionError {
    fn from(e: c_int) -> Self {
        match e {
            XCB_CONN_ERROR => XcbConnectionError::Io,
            XCB_CONN_CLOSED_EXT_NOTSUPPORTED => XcbConnectionError::UnsupportedExtension,
            XCB_CONN_CLOSED_MEM_INSUFFICIENT => XcbConnectionError::OutOfMemory,
            XCB_CONN_CLOSED_REQ_LEN_EXCEED => XcbConnectionError::MessageLength,
            XCB_CONN_CLOSED_PARSE_ERR => XcbConnectionError::DisplayString,
            XCB_CONN_CLOSED_INVALID_SCREEN => XcbConnectionError::InvalidScreen,
            XCB_CONN_CLOSED_FDPASSING_FAILED => XcbConnectionError::FileDescriptors,
            _ => XcbConnectionError::Unknown(e),
        }
    }
}

#[derive(Clone, Error, Debug)]
#[non_exhaustive]
pub enum XcbErrorType {
    #[error("An unknown error occurred: {0:?}")]
    Unknown(xcb_generic_error_t),
    #[error("The X server did not send a reply to a request")]
    MissingReply,
    #[error("The connection was terminated due to an error: {0}")]
    Connection(XcbConnectionError),
    #[error(transparent)]
    Core(core::CoreError),
    #[error("XVideo extension error: {0}")]
    Xv(xv::XvError),
    #[error("XFIXES extension error: {0}")]
    Xfixes(xfixes::XfixesError),
    #[error("SHM extension error: {0}")]
    Shm(shm::ShmError),
    #[error("DAMAGE extension error: {0}")]
    Damage(damage::DamageError),
    #[error("Print extension error: {0}")]
    XPrint(x_print::XPrintError),
    #[error("RANDR extension error: {0}")]
    Randr(randr::RandrError),
    #[error("RENDER extension error: {0}")]
    Render(render::RenderError),
    #[error("SYNC extension error: {0}")]
    Sync(sync::SyncError),
    #[error("RECORD extension error: {0}")]
    Record(record::RecordError),
    #[error("XKEYBOARD extension error: {0}")]
    Xkb(xkb::XkbError),
    #[error("GLX extension error: {0}")]
    Glx(glx::GlxError),
    #[error("XInput extension error: {0}")]
    Input(input::InputError),
}

#[derive(Debug)]
pub struct XcbErrorParser {
    pub(crate) c: *mut xcb_connection_t,
    parsers: Vec<ErrorParser>,
}

unsafe fn check_core_error(err: *mut xcb_generic_error_t) -> Result<(), XcbErrorType> {
    if err.is_null() {
        return Ok(());
    }
    let err = XcbBox::new(err);
    let mut error_code = err.error_code;
    assert!(error_code > 0);
    error_code -= 1;
    assert!(error_code < core::CONFIG.num_errors);
    Err((core::CONFIG.parse)(error_code, &*err))
}

impl XcbErrorParser {
    pub unsafe fn new(xcb: &Xcb, c: *mut xcb_connection_t) -> Self {
        let mut bases = HashMap::new();
        loop {
            let mut err = ptr::null_mut();
            let extensions = xcb.xcb_list_extensions_reply(c, xcb.xcb_list_extensions(c), &mut err);
            if let Err(e) = check_core_error(err) {
                log::error!("Could not list extensions: {}", e);
                break;
            }
            let extensions = XcbBox::new(extensions);
            let mut names_iter = xcb.xcb_list_extensions_names_iterator(&*extensions);
            while names_iter.rem > 0 {
                let name = xcb.xcb_str_name(names_iter.data);
                let len = (*names_iter.data).name_len;
                let ext = xcb.xcb_query_extension_reply(
                    c,
                    xcb.xcb_query_extension(c, len as _, name),
                    &mut err,
                );
                if let Err(e) = check_core_error(err) {
                    log::error!("Could not query extension: {}", e);
                    continue;
                }
                let ext = XcbBox::new(ext);
                let name = slice::from_raw_parts(name as *const u8, len as _);
                bases.insert(name, ext.first_error);
                xcb.xcb_str_next(&mut names_iter);
            }
            break;
        }

        let mut parsers = vec![];
        for config in CONFIGS {
            let min = match config.name {
                Some(name) => bases.get(name).cloned(),
                _ => Some(1),
            };
            if let Some(min) = min {
                parsers.push(ErrorParser {
                    min,
                    max_plus_1: min + config.num_errors,
                    config: *config,
                });
            }
        }
        parsers.sort_by_key(|p| p.min);
        for w in parsers.windows(2) {
            assert!(w[0].max_plus_1 <= w[1].min);
        }
        Self { c, parsers }
    }

    pub unsafe fn parse(&self, e: &xcb_generic_error_t) -> XcbError {
        let ty = 'outer: loop {
            for p in &self.parsers {
                if p.min <= e.error_code && e.error_code < p.max_plus_1 {
                    break 'outer (p.config.parse)(e.error_code - p.min, e);
                }
            }
            break XcbErrorType::Unknown(*e);
        };
        XcbError {
            error_code: e.error_code,
            sequence: e.full_sequence,
            major: e.major_code,
            minor: e.minor_code,
            ty,
        }
    }

    #[inline]
    pub unsafe fn check<T>(
        &self,
        xcb: &Xcb,
        t: *mut T,
        e: *mut xcb_generic_error_t,
    ) -> Result<XcbBox<T>, XcbError> {
        if !e.is_null() {
            let e = XcbBox::new(e);
            Err(self.parse(&e))
        } else {
            self.check_val(xcb, t)
        }
    }

    #[inline]
    pub unsafe fn check_val<T>(&self, xcb: &Xcb, t: *mut T) -> Result<XcbBox<T>, XcbError> {
        if t.is_null() {
            self.check_connection(xcb)
                .and(Err(XcbErrorType::MissingReply.into()))
        } else {
            Ok(XcbBox::new(t))
        }
    }

    #[inline]
    pub unsafe fn check_connection(&self, xcb: &Xcb) -> Result<(), XcbError> {
        let e = xcb.xcb_connection_has_error(self.c);
        if e == 0 {
            Ok(())
        } else {
            Err(XcbErrorType::Connection(e.into()).into())
        }
    }

    #[inline]
    pub unsafe fn check_err(&self, e: *mut xcb_generic_error_t) -> Result<(), XcbError> {
        if !e.is_null() {
            let e = XcbBox::new(e);
            Err(self.parse(&e))
        } else {
            Ok(())
        }
    }

    #[inline]
    pub unsafe fn check_cookie(
        &self,
        xcb: &Xcb,
        cookie: xcb_void_cookie_t,
    ) -> Result<(), XcbError> {
        let err = xcb.xcb_request_check(self.c, cookie);
        self.check_err(err)
    }
}

struct ErrorParser {
    min: u8,
    max_plus_1: u8,
    config: &'static ErrorConfig,
}

impl Debug for ErrorParser {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let name = self.config.name.unwrap_or(b"Core");
        write!(
            f,
            "{}([{}, {}])",
            name.as_bstr(),
            self.min,
            self.max_plus_1 - 1
        )
    }
}

struct ErrorConfig {
    name: Option<&'static [u8]>,
    num_errors: u8,
    parse: unsafe fn(error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType,
}

const CONFIGS: &[&ErrorConfig] = &[
    &core::CONFIG,
    &xv::CONFIG,
    &xfixes::CONFIG,
    &shm::CONFIG,
    &damage::CONFIG,
    &x_print::CONFIG,
    &randr::CONFIG,
    &render::CONFIG,
    &sync::CONFIG,
    &record::CONFIG,
    &xkb::CONFIG,
    &glx::CONFIG,
    &input::CONFIG,
];

pub mod core {
    use super::*;

    #[derive(Clone, Debug, Error)]
    #[error("{ty} (major: {major_opcode}, minor: {minor_opcode})")]
    pub struct CoreError {
        pub major_opcode: u8,
        pub minor_opcode: u16,
        pub ty: CoreErrorType,
    }

    #[derive(Clone, Debug)]
    pub struct RequestError {
        pub bad_value: u32,
    }

    #[derive(Clone, Debug)]
    pub struct ValueError {
        pub bad_value: u32,
    }

    #[derive(Clone, Debug, Error)]
    pub enum CoreErrorType {
        #[error("Request error (bad value: {})", .0.bad_value)]
        Request(RequestError),
        #[error("Value error (bad value: {})", .0.bad_value)]
        Value(ValueError),
        #[error("Window error (bad value: {})", .0.bad_value)]
        Window(ValueError),
        #[error("Pixmap error (bad value: {})", .0.bad_value)]
        Pixmap(ValueError),
        #[error("Atom error (bad value: {})", .0.bad_value)]
        Atom(ValueError),
        #[error("Cursor error (bad value: {})", .0.bad_value)]
        Cursor(ValueError),
        #[error("Font error (bad value: {})", .0.bad_value)]
        Font(ValueError),
        #[error("Match error (bad value: {})", .0.bad_value)]
        Match(RequestError),
        #[error("Drawable error (bad value: {})", .0.bad_value)]
        Drawable(ValueError),
        #[error("Access error (bad value: {})", .0.bad_value)]
        Access(RequestError),
        #[error("Alloc error (bad value: {})", .0.bad_value)]
        Alloc(RequestError),
        #[error("Colormap error (bad value: {})", .0.bad_value)]
        Colormap(ValueError),
        #[error("GContext error (bad value: {})", .0.bad_value)]
        GContext(ValueError),
        #[error("IDChoice error (bad value: {})", .0.bad_value)]
        IDChoice(ValueError),
        #[error("Name error (bad value: {})", .0.bad_value)]
        Name(RequestError),
        #[error("Length error (bad value: {})", .0.bad_value)]
        Length(RequestError),
        #[error("Implementation error (bad value: {})", .0.bad_value)]
        Implementation(RequestError),
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: None,
        num_errors: 17,
        parse,
    };

    unsafe fn parse(error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = &*(e as *const xcb_request_error_t);
        let ty = match error_code {
            0 => CoreErrorType::Request(RequestError {
                bad_value: e.bad_value,
            }),
            1 => CoreErrorType::Value(ValueError {
                bad_value: e.bad_value,
            }),
            2 => CoreErrorType::Window(ValueError {
                bad_value: e.bad_value,
            }),
            3 => CoreErrorType::Pixmap(ValueError {
                bad_value: e.bad_value,
            }),
            4 => CoreErrorType::Atom(ValueError {
                bad_value: e.bad_value,
            }),
            5 => CoreErrorType::Cursor(ValueError {
                bad_value: e.bad_value,
            }),
            6 => CoreErrorType::Font(ValueError {
                bad_value: e.bad_value,
            }),
            7 => CoreErrorType::Match(RequestError {
                bad_value: e.bad_value,
            }),
            8 => CoreErrorType::Drawable(ValueError {
                bad_value: e.bad_value,
            }),
            9 => CoreErrorType::Access(RequestError {
                bad_value: e.bad_value,
            }),
            10 => CoreErrorType::Alloc(RequestError {
                bad_value: e.bad_value,
            }),
            11 => CoreErrorType::Colormap(ValueError {
                bad_value: e.bad_value,
            }),
            12 => CoreErrorType::GContext(ValueError {
                bad_value: e.bad_value,
            }),
            13 => CoreErrorType::IDChoice(ValueError {
                bad_value: e.bad_value,
            }),
            14 => CoreErrorType::Name(RequestError {
                bad_value: e.bad_value,
            }),
            15 => CoreErrorType::Length(RequestError {
                bad_value: e.bad_value,
            }),
            16 => CoreErrorType::Implementation(RequestError {
                bad_value: e.bad_value,
            }),
            _ => unreachable!(),
        };
        XcbErrorType::Core(CoreError {
            major_opcode: e.major_opcode,
            minor_opcode: e.minor_opcode,
            ty,
        })
    }
}

pub mod xv {
    use super::*;

    #[derive(Clone, Debug, Error)]
    pub enum XvError {
        #[error("Bad port")]
        BadPort,
        #[error("Bad encoding")]
        BadEncoding,
        #[error("Bad control")]
        BadControl,
    }

    const XCB_XV_NAME: &[u8] = b"XVideo";

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_XV_NAME),
        num_errors: 3,
        parse,
    };

    unsafe fn parse(error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = match error_code {
            0 => XvError::BadPort,
            1 => XvError::BadEncoding,
            2 => XvError::BadControl,
            _ => unreachable!(),
        };
        XcbErrorType::Xv(e)
    }
}

pub mod xfixes {
    use super::*;

    const XCB_XFIXES_NAME: &[u8] = b"XFIXES";

    #[derive(Clone, Debug, Error)]
    pub enum XfixesError {
        #[error("Bad region")]
        BadRegion,
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_XFIXES_NAME),
        num_errors: 1,
        parse,
    };

    unsafe fn parse(_error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        XcbErrorType::Xfixes(XfixesError::BadRegion)
    }
}

pub mod shm {
    use super::*;
    use crate::error::core::ValueError;

    const XCB_SHM_NAME: &[u8] = b"MIT-SHM";

    #[repr(C)]
    struct xcb_value_error_t {
        pub response_type: u8,
        pub error_code: u8,
        pub sequence: u16,
        pub bad_value: u32,
        pub minor_opcode: u16,
        pub major_opcode: u8,
        pub pad0: u8,
    }

    type xcb_shm_bad_seg_error_t = xcb_value_error_t;

    #[derive(Clone, Debug, Error)]
    #[error("{ty} (major: {major_opcode}, minor: {minor_opcode})")]
    pub struct ShmError {
        pub major_opcode: u8,
        pub minor_opcode: u16,
        pub ty: ShmErrorType,
    }

    #[derive(Clone, Debug, Error)]
    pub enum ShmErrorType {
        #[error("Bad segment (bad value: {})", .0.bad_value)]
        BadSeg(ValueError),
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_SHM_NAME),
        num_errors: 1,
        parse,
    };

    unsafe fn parse(_error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = &*(e as *const xcb_shm_bad_seg_error_t);
        XcbErrorType::Shm(ShmError {
            major_opcode: e.major_opcode,
            minor_opcode: e.minor_opcode,
            ty: ShmErrorType::BadSeg(ValueError {
                bad_value: e.bad_value,
            }),
        })
    }
}

pub mod damage {
    use super::*;

    const XCB_DAMAGE_NAME: &[u8] = b"DAMAGE";

    #[derive(Clone, Debug, Error)]
    pub enum DamageError {
        #[error("Bad damage")]
        BadDamage,
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_DAMAGE_NAME),
        num_errors: 1,
        parse,
    };

    unsafe fn parse(_error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        XcbErrorType::Damage(DamageError::BadDamage)
    }
}

pub mod x_print {
    use super::*;

    const XCB_X_PRINT_NAME: &[u8] = b"XpExtension";

    #[derive(Clone, Debug, Error)]
    pub enum XPrintError {
        #[error("Bad context")]
        BadContext,
        #[error("Bad sequence")]
        BadSequence,
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_X_PRINT_NAME),
        num_errors: 2,
        parse,
    };

    unsafe fn parse(error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = match error_code {
            0 => XPrintError::BadContext,
            1 => XPrintError::BadSequence,
            _ => unreachable!(),
        };
        XcbErrorType::XPrint(e)
    }
}

pub mod randr {
    use super::*;

    const XCB_RANDR_NAME: &[u8] = b"RANDR";

    #[derive(Clone, Debug, Error)]
    pub enum RandrError {
        #[error("Bad output")]
        BadOutput,
        #[error("Bad crtc")]
        BadCrtc,
        #[error("Bad mode")]
        BadMode,
        #[error("Bad provider")]
        BadProvider,
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_RANDR_NAME),
        num_errors: 4,
        parse,
    };

    unsafe fn parse(error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = match error_code {
            0 => RandrError::BadOutput,
            1 => RandrError::BadCrtc,
            2 => RandrError::BadMode,
            3 => RandrError::BadProvider,
            _ => unreachable!(),
        };
        XcbErrorType::Randr(e)
    }
}

pub mod render {
    use super::*;

    const XCB_RENDER_NAME: &[u8] = b"RENDER";

    #[derive(Clone, Debug, Error)]
    pub enum RenderError {
        #[error("Invalid picture format")]
        PictFormat,
        #[error("Invalid picture")]
        Picture,
        #[error("Invalid picture operation")]
        PictOp,
        #[error("Invalid glyph set")]
        GlyphSet,
        #[error("Invalid glyph")]
        Glyph,
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_RENDER_NAME),
        num_errors: 5,
        parse,
    };

    unsafe fn parse(error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = match error_code {
            0 => RenderError::PictFormat,
            1 => RenderError::Picture,
            2 => RenderError::PictOp,
            3 => RenderError::GlyphSet,
            4 => RenderError::Glyph,
            _ => unreachable!(),
        };
        XcbErrorType::Render(e)
    }
}

pub mod sync {
    use super::*;

    const XCB_SYNC_NAME: &[u8] = b"SYNC";

    #[repr(C)]
    struct xcb_sync_counter_error_t {
        pub response_type: u8,
        pub error_code: u8,
        pub sequence: u16,
        pub bad_counter: u32,
        pub minor_opcode: u16,
        pub major_opcode: u8,
    }

    #[derive(Clone, Debug)]
    pub struct CounterError {
        pub bad_counter: u32,
    }

    #[derive(Clone, Debug)]
    pub struct AlarmError {
        pub bad_alarm: u32,
    }

    #[derive(Clone, Debug, Error)]
    #[error("{ty} (major: {major_opcode}, minor: {minor_opcode})")]
    pub struct SyncError {
        pub major_opcode: u8,
        pub minor_opcode: u16,
        pub ty: SyncErrorType,
    }

    #[derive(Clone, Debug, Error)]
    pub enum SyncErrorType {
        #[error("Bad counter {}", .0.bad_counter)]
        Counter(CounterError),
        #[error("Bad alarm {}", .0.bad_alarm)]
        Alarm(AlarmError),
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_SYNC_NAME),
        num_errors: 2,
        parse,
    };

    unsafe fn parse(error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = &*(e as *const xcb_sync_counter_error_t);
        let ty = match error_code {
            0 => SyncErrorType::Counter(CounterError {
                bad_counter: e.bad_counter,
            }),
            1 => SyncErrorType::Alarm(AlarmError {
                bad_alarm: e.bad_counter,
            }),
            _ => unreachable!(),
        };
        XcbErrorType::Sync(SyncError {
            major_opcode: e.major_opcode,
            minor_opcode: e.minor_opcode,
            ty,
        })
    }
}

pub mod record {
    use super::*;

    const XCB_RECORD_NAME: &[u8] = b"RECORD";

    #[repr(C)]
    struct xcb_record_bad_context_error_t {
        pub response_type: u8,
        pub error_code: u8,
        pub sequence: u16,
        pub invalid_record: u32,
    }

    #[derive(Clone, Debug)]
    pub struct BadContext {
        pub invalid_record: u32,
    }

    #[derive(Clone, Debug, Error)]
    pub enum RecordError {
        #[error("Bad context (invalid record: {})", .0.invalid_record)]
        BadContext(BadContext),
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_RECORD_NAME),
        num_errors: 1,
        parse,
    };

    unsafe fn parse(_error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = &*(e as *const xcb_record_bad_context_error_t);
        XcbErrorType::Record(RecordError::BadContext(BadContext {
            invalid_record: e.invalid_record,
        }))
    }
}

pub mod xkb {
    use super::*;

    const XCB_XKB_NAME: &[u8] = b"XKEYBOARD";

    #[repr(C)]
    struct xcb_xkb_keyboard_error_t {
        pub response_type: u8,
        pub error_code: u8,
        pub sequence: u16,
        pub value: u32,
        pub minor_opcode: u16,
        pub major_opcode: u8,
        pub pad0: [u8; 21],
    }

    bitflags::bitflags! {
        pub struct XkbKeyboardError: u32 {
            const BAD_DEVICE = 255;
            const BAD_CLASS = 254;
            const BAD_ID = 253;
        }
    }

    #[derive(Clone, Debug, Error)]
    #[error("{ty} (major: {major_opcode}, minor: {minor_opcode})")]
    pub struct XkbError {
        pub major_opcode: u8,
        pub minor_opcode: u16,
        pub ty: XkbErrorType,
    }

    #[derive(Clone, Debug, Error)]
    pub enum XkbErrorType {
        #[error("Keyboard error: {0:?}")]
        Keyboard(XkbKeyboardError),
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_XKB_NAME),
        num_errors: 1,
        parse,
    };

    unsafe fn parse(_error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = &*(e as *const xcb_xkb_keyboard_error_t);
        XcbErrorType::Xkb(XkbError {
            major_opcode: e.major_opcode,
            minor_opcode: e.minor_opcode,
            ty: XkbErrorType::Keyboard(XkbKeyboardError::from_bits_unchecked(e.value)),
        })
    }
}

pub mod glx {
    use super::*;

    const XCB_GLX_NAME: &[u8] = b"GLX";

    #[repr(C)]
    struct xcb_glx_generic_error_t {
        pub response_type: u8,
        pub error_code: u8,
        pub sequence: u16,
        pub bad_value: u32,
        pub minor_opcode: u16,
        pub major_opcode: u8,
        pub pad0: [u8; 21],
    }

    #[derive(Clone, Debug)]
    pub struct GenericError {
        pub bad_value: u32,
    }

    #[derive(Clone, Debug, Error)]
    #[error("{ty} (major: {major_opcode}, minor: {minor_opcode})")]
    pub struct GlxError {
        pub major_opcode: u8,
        pub minor_opcode: u16,
        pub ty: GlxErrorType,
    }

    #[derive(Clone, Debug, Error)]
    pub enum GlxErrorType {
        #[error("Bad context (bad value: {})", .0.bad_value)]
        BadContext(GenericError),
        #[error("Bad context state (bad value: {})", .0.bad_value)]
        BadContextState(GenericError),
        #[error("Bad drawable (bad value: {})", .0.bad_value)]
        BadDrawable(GenericError),
        #[error("Bad pixmap (bad value: {})", .0.bad_value)]
        BadPixmap(GenericError),
        #[error("Bad context tag (bad value: {})", .0.bad_value)]
        BadContextTag(GenericError),
        #[error("Bad current window (bad value: {})", .0.bad_value)]
        BadCurrentWindow(GenericError),
        #[error("Bad render request (bad value: {})", .0.bad_value)]
        BadRenderRequest(GenericError),
        #[error("Bad large request (bad value: {})", .0.bad_value)]
        BadLargeRequest(GenericError),
        #[error("Unsupported private request (bad value: {})", .0.bad_value)]
        UnsupportedPrivateRequest(GenericError),
        #[error("Bad framebuffer config (bad value: {})", .0.bad_value)]
        BadFBConfig(GenericError),
        #[error("Bad pixel buffer (bad value: {})", .0.bad_value)]
        BadPbuffer(GenericError),
        #[error("Bad current drawable (bad value: {})", .0.bad_value)]
        BadCurrentDrawable(GenericError),
        #[error("Bad window (bad value: {})", .0.bad_value)]
        BadWindow(GenericError),
        #[error("Bad profile (bad value: {})", .0.bad_value)]
        GLXBadProfileARB(GenericError),
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_GLX_NAME),
        num_errors: 14,
        parse,
    };

    unsafe fn parse(error_code: u8, e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = &*(e as *const xcb_glx_generic_error_t);
        let ge = GenericError {
            bad_value: e.bad_value,
        };
        let ty = match error_code {
            0 => GlxErrorType::BadContext(ge),
            1 => GlxErrorType::BadContextState(ge),
            2 => GlxErrorType::BadDrawable(ge),
            3 => GlxErrorType::BadPixmap(ge),
            4 => GlxErrorType::BadContextTag(ge),
            5 => GlxErrorType::BadCurrentWindow(ge),
            6 => GlxErrorType::BadRenderRequest(ge),
            7 => GlxErrorType::BadLargeRequest(ge),
            8 => GlxErrorType::UnsupportedPrivateRequest(ge),
            9 => GlxErrorType::BadFBConfig(ge),
            10 => GlxErrorType::BadPbuffer(ge),
            11 => GlxErrorType::BadCurrentDrawable(ge),
            12 => GlxErrorType::BadWindow(ge),
            13 => GlxErrorType::GLXBadProfileARB(ge),
            _ => unreachable!(),
        };
        XcbErrorType::Glx(GlxError {
            major_opcode: e.major_opcode,
            minor_opcode: e.minor_opcode,
            ty,
        })
    }
}

pub mod input {
    use super::*;

    const XCB_INPUT_NAME: &[u8] = b"XInputExtension";

    #[derive(Clone, Debug, Error)]
    pub enum InputError {
        #[error("Bad device")]
        Device,
        #[error("Bad event")]
        Event,
        #[error("Bad mode")]
        Mode,
        #[error("Device busy")]
        DeviceBusy,
        #[error("Bad class")]
        Class,
    }

    pub(super) const CONFIG: ErrorConfig = ErrorConfig {
        name: Some(XCB_INPUT_NAME),
        num_errors: 5,
        parse,
    };

    unsafe fn parse(error_code: u8, _e: *const xcb_generic_error_t) -> XcbErrorType {
        let e = match error_code {
            0 => InputError::Device,
            1 => InputError::Event,
            2 => InputError::Mode,
            3 => InputError::DeviceBusy,
            4 => InputError::Class,
            _ => unreachable!(),
        };
        XcbErrorType::Input(e)
    }
}
