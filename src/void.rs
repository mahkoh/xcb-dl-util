use crate::error::{XcbError, XcbErrorParser};
use smallvec::SmallVec;
use std::mem::ManuallyDrop;
use xcb_dl::ffi::*;
use xcb_dl::Xcb;

#[must_use = "XcbPendingCommand panics when dropped."]
pub struct XcbPendingCommand {
    cookie: xcb_void_cookie_t,
}

impl XcbPendingCommand {
    pub fn new(cookie: xcb_void_cookie_t) -> Self {
        Self { cookie }
    }

    pub unsafe fn check(self, xcb: &Xcb, errors: &XcbErrorParser) -> Result<(), XcbError> {
        let slf = ManuallyDrop::new(self);
        errors.check_cookie(xcb, slf.cookie)
    }

    pub unsafe fn discard(self, xcb: &Xcb, c: *mut xcb_connection_t) {
        let slf = ManuallyDrop::new(self);
        xcb.xcb_discard_reply(c, slf.cookie.sequence);
    }

    pub fn and_then(self, command: XcbPendingCommand) -> XcbPendingCommands {
        let slf = ManuallyDrop::new(self);
        let command = ManuallyDrop::new(command);
        XcbPendingCommands {
            cookies: smallvec::smallvec![slf.cookie, command.cookie],
        }
    }
}
//
// impl IntoIterator for XcbPendingCommand {
//     type Item = xcb_void_cookie_t;
//     type IntoIter = std::iter::Once<xcb_void_cookie_t>;
//
//     fn into_iter(self) -> Self::IntoIter {
//         std::iter::once()
//     }
// }

impl From<xcb_void_cookie_t> for XcbPendingCommand {
    fn from(c: xcb_void_cookie_t) -> Self {
        Self { cookie: c }
    }
}

impl Drop for XcbPendingCommand {
    fn drop(&mut self) {
        panic!("XcbPendingCommand was not handled. You must call `check` or `discard` instead of dropping this type.");
    }
}

#[must_use = "XcbPendingCommands panics when dropped."]
pub struct XcbPendingCommands {
    cookies: SmallVec<[xcb_void_cookie_t; 3]>,
}

impl XcbPendingCommands {
    pub fn new() -> Self {
        Self {
            cookies: Default::default(),
        }
    }

    pub unsafe fn check(self, xcb: &Xcb, errors: &XcbErrorParser) -> Result<(), XcbError> {
        let slf = ManuallyDrop::new(self);
        let mut i = 0;
        let mut err = Ok(());
        while i < slf.cookies.len() && err.is_ok() {
            err = errors.check_cookie(xcb, slf.cookies[i]);
            i += 1;
        }
        while i < slf.cookies.len() {
            xcb.xcb_discard_reply(errors.c, slf.cookies[i].sequence);
            i += 1;
        }
        err
    }

    pub unsafe fn discard(self, xcb: &Xcb, c: *mut xcb_connection_t) {
        let slf = ManuallyDrop::new(self);
        for cookie in &slf.cookies {
            xcb.xcb_discard_reply(c, cookie.sequence);
        }
    }

    pub fn extend(&mut self, commands: XcbPendingCommands) {
        let commands = ManuallyDrop::new(commands);
        self.cookies.extend_from_slice(&commands.cookies);
    }

    pub fn push(&mut self, command: XcbPendingCommand) {
        let command = ManuallyDrop::new(command);
        self.cookies.push(command.cookie);
    }
}

impl From<XcbPendingCommand> for XcbPendingCommands {
    fn from(p: XcbPendingCommand) -> Self {
        let p = ManuallyDrop::new(p);
        Self {
            cookies: smallvec::smallvec!(p.cookie),
        }
    }
}

impl Drop for XcbPendingCommands {
    fn drop(&mut self) {
        panic!("XcbPendingCommand was not handled. You must call `check` or `discard` instead of dropping this type.");
    }
}
