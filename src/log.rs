use crate::xcb_box::XcbBox;
use bstr::ByteSlice;
use std::{ptr, slice};
use xcb_dl::ffi::*;
use xcb_dl::Xcb;

pub unsafe fn log_connection(level: log::Level, xcb: &Xcb, c: *mut xcb_connection_t) {
    if !log::log_enabled!(level) {
        return;
    }
    let setup = &*xcb.xcb_get_setup(c);
    let vendor = std::slice::from_raw_parts(
        xcb.xcb_setup_vendor(setup) as *const u8,
        setup.vendor_len as _,
    )
    .as_bstr();
    let mut screens = xcb.xcb_setup_roots_iterator(setup);
    log::log!(
        level,
        "X server protocol version: {}.{} (release {})",
        setup.protocol_major_version,
        setup.protocol_minor_version,
        setup.release_number
    );
    log::log!(level, "  Vendor: {}", vendor);
    log::log!(
        level,
        "  Maximum request length: {}",
        setup.maximum_request_length
    );
    log::log!(level, "  Screens:");
    while screens.rem > 0 {
        let screen = &*screens.data;
        log::log!(
            level,
            "    - Size: {}x{}",
            screen.width_in_pixels,
            screen.height_in_pixels
        );
        log::log!(level, "      Root depth: {}", screen.root_depth);
        let mut depths = xcb.xcb_screen_allowed_depths_iterator(screen);
        let mut d = vec![];
        while depths.rem > 0 {
            let depth = &*depths.data;
            d.push(depth.depth);
            xcb.xcb_depth_next(&mut depths);
        }
        d.sort();
        log::log!(level, "      Allowed depths: {:?}", d);
        xcb.xcb_screen_next(&mut screens);
    }

    loop {
        let mut err = ptr::null_mut();
        let extensions = xcb.xcb_list_extensions_reply(c, xcb.xcb_list_extensions(c), &mut err);
        if !err.is_null() {
            XcbBox::new(err);
            break;
        }
        let extensions = XcbBox::new(extensions);
        let mut e = vec![];
        let mut names_iter = xcb.xcb_list_extensions_names_iterator(&*extensions);
        while names_iter.rem > 0 {
            let name = slice::from_raw_parts(
                xcb.xcb_str_name(names_iter.data) as *const u8,
                (*names_iter.data).name_len as _,
            )
            .as_bstr();
            e.push(name);
            xcb.xcb_str_next(&mut names_iter);
        }
        e.sort();
        log::log!(level, "  Extensions:");
        for e in e {
            log::log!(level, "    - {}", e);
        }
        break;
    }
}
