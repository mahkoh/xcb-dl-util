use xcb_dl::ffi::*;
use xcb_dl::XcbXinput;

#[repr(C)]
struct Mask<const N: usize> {
    head: xcb_input_event_mask_t,
    mask: [u32; N],
}

#[inline]
pub unsafe fn select_events<const N: usize>(
    xinput: &XcbXinput,
    c: *mut xcb_connection_t,
    window: xcb_window_t,
    device_id: xcb_input_device_id_t,
    mask: [u32; N],
) -> xcb_void_cookie_t {
    let mask = Mask {
        head: xcb_input_event_mask_t {
            deviceid: device_id,
            mask_len: N as _,
        },
        mask,
    };
    xinput.xcb_input_xi_select_events(c, window, 1, &mask.head)
}

#[inline]
pub unsafe fn select_events_checked<const N: usize>(
    xinput: &XcbXinput,
    c: *mut xcb_connection_t,
    window: xcb_window_t,
    device_id: xcb_input_device_id_t,
    mask: [u32; N],
) -> xcb_void_cookie_t {
    let mask = Mask {
        head: xcb_input_event_mask_t {
            deviceid: device_id,
            mask_len: N as _,
        },
        mask,
    };
    xinput.xcb_input_xi_select_events_checked(c, window, 1, &mask.head)
}
