#[cfg(feature = "xcb_render")]
pub mod cursor;
pub mod error;
pub mod format;
pub mod hint;
#[cfg(feature = "xcb_xinput")]
pub mod input;
pub mod log;
pub mod property;
#[cfg(feature = "xcb_render")]
pub mod render;
pub mod void;
pub mod xcb_box;

#[test]
fn test() {
    use crate::error::*;
    use std::*;
    use xcb_dl::ffi::*;
    use xcb_dl::*;
    unsafe {
        println!("a");
        let xcb = Xcb::load().unwrap();
        let c = xcb.xcb_connect(ptr::null(), ptr::null_mut());
        let randr = XcbRandr::load().unwrap();
        randr.xcb_randr_query_version_unchecked(c, 1, 3);
        let setup = xcb.xcb_get_setup(c);
        let screen = *xcb.xcb_setup_roots_iterator(setup).data;
        let rep = randr.xcb_randr_get_screen_resources_reply(
            c,
            randr.xcb_randr_get_screen_resources(c, screen.root),
            ptr::null_mut(),
        );
        let crtc = *randr.xcb_randr_get_screen_resources_crtcs(rep);
        let info = &*randr.xcb_randr_get_crtc_info_reply(
            c,
            randr.xcb_randr_get_crtc_info(c, crtc, 0),
            ptr::null_mut(),
        );
        println!("a");
        let cookie = randr.xcb_randr_set_crtc_config(
            c,
            crtc,
            XCB_TIME_CURRENT_TIME,
            // See the comment in get_crtc_info.
            0,
            info.x,
            info.y,
            info.mode,
            info.rotation,
            info.num_outputs as _,
            randr.xcb_randr_get_crtc_info_outputs(info),
        );
        let mut err = ptr::null_mut();
        let reply = randr.xcb_randr_set_crtc_config_reply(c, cookie, &mut err);
        println!("a");
        let errors = XcbErrorParser::new(&xcb, c);
        println!("a");
        println!("{:#?}", errors.check(&xcb, reply, err));
    }
}
