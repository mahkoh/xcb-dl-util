use std::convert::TryFrom;
use std::{mem, ptr};
use thiserror::Error;
use xcb_dl::ffi::*;

bitflags::bitflags! {
    #[derive(Default)]
    pub struct XcbSizeHintsFlags: u32 {
        const U_POSITION = 1 << 0;
        const U_SIZE = 1 << 1;
        const P_POSITION = 1 << 2;
        const P_SIZE = 1 << 3;
        const P_MIN_SIZE = 1 << 4;
        const P_MAX_SIZE = 1 << 5;
        const P_RESIZE_INCREMENT = 1 << 6;
        const P_ASPECT_RATIOS = 1 << 7;
        const P_BASE_SIZE = 1 << 8;
        const P_WINDOW_GRAVITY = 1 << 9;
    }

    #[derive(Default)]
    pub struct XcbGravity: u32 {
        const WIN_UNMAP  = XCB_GRAVITY_WIN_UNMAP;
        const NORTH_WEST = XCB_GRAVITY_NORTH_WEST;
        const NORTH      = XCB_GRAVITY_NORTH;
        const NORTH_EAST = XCB_GRAVITY_NORTH_EAST;
        const WEST       = XCB_GRAVITY_WEST;
        const CENTER     = XCB_GRAVITY_CENTER;
        const EAST       = XCB_GRAVITY_EAST;
        const SOUTH_WEST = XCB_GRAVITY_SOUTH_WEST;
        const SOUTH      = XCB_GRAVITY_SOUTH;
        const SOUTH_EAST = XCB_GRAVITY_SOUTH_EAST;
        const STATIC     = XCB_GRAVITY_STATIC;
    }
}

impl From<u32> for XcbGravity {
    fn from(v: u32) -> Self {
        unsafe { Self::from_bits_unchecked(v) }
    }
}

impl Into<u32> for XcbGravity {
    fn into(self) -> u32 {
        self.bits
    }
}

#[derive(Debug, Copy, Clone, Default)]
#[repr(C)]
pub struct XcbAspect {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Copy, Clone, Default)]
#[repr(C)]
pub struct XcbSizeHints {
    pub flags: XcbSizeHintsFlags,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub max_width: u32,
    pub max_height: u32,
    pub width_inc: u32,
    pub height_inc: u32,
    pub min_aspect: XcbAspect,
    pub max_aspect: XcbAspect,
    pub base_width: u32,
    pub base_height: u32,
    pub win_gravity: XcbGravity,
}

const SIZE_HINTS_LEN: usize = mem::size_of::<XcbSizeHints>() / 4;

// Compile time checks
const _SIZE_HINTS_REM: [usize; mem::size_of::<XcbSizeHints>() % 4] = [];
const _SIZE_HINTS_ALIGN: [usize; mem::align_of::<XcbSizeHints>() - mem::align_of::<u32>()] = [];

macro_rules! field {
    ($get:ident, $set:ident, ($($t:ty),*), ($($f:ident),*), $flags:expr) => {
        field!($get, $set, ($($t),*), ($($f),*), $flags, $flags);
    };
    ($get:ident, $set:ident, ($($t:ty),*), ($($f:ident),*), $flags:expr, $set_flags:expr) => {
        #[allow(unused_parens)]
        pub fn $get(&self) -> Option<($($t),*)> {
            if self.flags.intersects($flags) {
                Some(($(self.$f),*))
            } else {
                None
            }
        }

        #[allow(unused_parens)]
        pub fn $set(&mut self, o: Option<($($t),*)>) {
            if let Some(($($f),*)) = o {
                self.flags |= $set_flags;
                $(self.$f = $f;)*
            } else {
                self.flags &= !$flags;
                $(self.$f = Default::default();)*
            }
        }
    };
}

impl XcbSizeHints {
    pub fn as_bytes(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self as *const _ as _, SIZE_HINTS_LEN) }
    }

    field!(
        position,
        set_position,
        (i32, i32),
        (x, y),
        XcbSizeHintsFlags::P_POSITION | XcbSizeHintsFlags::U_POSITION,
        XcbSizeHintsFlags::P_POSITION
    );
    field!(
        size,
        set_size,
        (u32, u32),
        (width, height),
        XcbSizeHintsFlags::P_SIZE | XcbSizeHintsFlags::U_SIZE,
        XcbSizeHintsFlags::P_SIZE
    );
    field!(
        min_size,
        set_min_size,
        (u32, u32),
        (min_width, min_height),
        XcbSizeHintsFlags::P_MIN_SIZE
    );
    field!(
        max_size,
        set_max_size,
        (u32, u32),
        (max_width, max_height),
        XcbSizeHintsFlags::P_MAX_SIZE
    );
    field!(
        resize_increments,
        set_resize_increments,
        (u32, u32),
        (width_inc, height_inc),
        XcbSizeHintsFlags::P_RESIZE_INCREMENT
    );
    field!(
        aspect_ratios,
        set_aspect_ratios,
        (XcbAspect, XcbAspect),
        (min_aspect, max_aspect),
        XcbSizeHintsFlags::P_ASPECT_RATIOS
    );
    field!(
        base_size,
        set_base_size,
        (u32, u32),
        (base_width, base_height),
        XcbSizeHintsFlags::P_BASE_SIZE
    );
    field!(
        win_gravity,
        set_win_gravity,
        (XcbGravity),
        (win_gravity),
        XcbSizeHintsFlags::P_WINDOW_GRAVITY
    );
}

#[derive(Clone, Debug, Error)]
pub enum XcbSizeHintsError {
    #[error("The data is too small to be an XcbSizeHints object")]
    WrongSize,
}

impl<'a> TryFrom<&'a [u32]> for XcbSizeHints {
    type Error = XcbSizeHintsError;

    fn try_from(value: &'a [u32]) -> Result<Self, Self::Error> {
        if value.len() != SIZE_HINTS_LEN {
            return Err(XcbSizeHintsError::WrongSize);
        }
        unsafe { Ok(ptr::read(value.as_ptr() as *const XcbSizeHints)) }
    }
}

bitflags::bitflags! {
    #[derive(Default)]
    pub struct XcbHintsFlags: u32 {
        const INPUT = 1 << 0;
        const STATE = 1 << 1;
        const ICON_PIXMAP = 1 << 2;
        const ICON_WINDOW = 1 << 3;
        const P_SIZE = 1 << 4;
        const ICON_POSITION = 1 << 5;
        const WINDOW_GROUP = 1 << 6;
        const MESSAGE = 1 << 7;
        const URGENCY = 1 << 8;
    }
}

#[derive(Debug, Copy, Clone, Default)]
#[repr(C)]
pub struct XcbHints {
    pub flags: XcbHintsFlags,
    pub input: u32,
    pub initial_state: u32,
    pub icon_pixmap: xcb_pixmap_t,
    pub icon_window: xcb_window_t,
    pub icon_x: i32,
    pub icon_y: i32,
    pub icon_mask: xcb_pixmap_t,
    pub window_group: xcb_window_t,
}

const HINTS_LEN: usize = mem::size_of::<XcbHints>() / 4;

// Compile time checks
const _HINTS_REM: [usize; mem::size_of::<XcbHints>() % 4] = [];
const _HINTS_ALIGN: [usize; mem::align_of::<XcbHints>() - mem::align_of::<u32>()] = [];

impl XcbHints {
    pub fn as_bytes(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self as *const _ as _, HINTS_LEN) }
    }

    pub fn urgency(&mut self) -> bool {
        self.flags.contains(XcbHintsFlags::URGENCY)
    }

    pub fn set_urgency(&mut self, urgency: bool) {
        if urgency {
            self.flags |= XcbHintsFlags::URGENCY
        } else {
            self.flags &= !XcbHintsFlags::URGENCY
        }
    }

    field!(input, set_input, (u32), (input), XcbHintsFlags::INPUT);
    field!(
        initial_state,
        set_initial_state,
        (u32),
        (input),
        XcbHintsFlags::STATE
    );
    field!(
        icon_pixmap,
        set_icon_pixmap,
        (xcb_pixmap_t),
        (icon_pixmap),
        XcbHintsFlags::ICON_PIXMAP
    );
    field!(
        icon_window,
        set_icon_window,
        (xcb_window_t),
        (icon_window),
        XcbHintsFlags::ICON_WINDOW
    );
    field!(
        icon_position,
        set_icon_position,
        (i32, i32),
        (icon_x, icon_y),
        XcbHintsFlags::ICON_POSITION
    );
    field!(
        window_group,
        set_window_group,
        (xcb_window_t),
        (window_group),
        XcbHintsFlags::WINDOW_GROUP
    );
}

#[derive(Clone, Debug, Error)]
pub enum XcbHintsError {
    #[error("The data is too small to be an XcbHints object")]
    WrongSize,
}

impl<'a> TryFrom<&'a [u32]> for XcbHints {
    type Error = XcbHintsError;

    fn try_from(value: &'a [u32]) -> Result<Self, Self::Error> {
        if value.len() != HINTS_LEN {
            return Err(XcbHintsError::WrongSize);
        }
        unsafe { Ok(ptr::read(value.as_ptr() as *const XcbHints)) }
    }
}
