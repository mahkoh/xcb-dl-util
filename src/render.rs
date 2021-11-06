use xcb_dl::ffi::*;
use xcb_dl::XcbRender;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum XcbPictFormat {
    Argb32,
    Rgb24,
    A8,
    A4,
    A1,
}

bitflags::bitflags! {
    pub struct XcbPictFormatFeatures: u16 {
        const ID = 1 << 0;
        const TYPE = 1 << 1;
        const DEPTH = 1 << 2;
        const RED_SHIFT = 1 << 3;
        const RED_MASK = 1 << 4;
        const GREEN_SHIFT = 1 << 5;
        const GREEN_MASK = 1 << 6;
        const BLUE_SHIFT = 1 << 7;
        const BLUE_MASK = 1 << 8;
        const ALPHA_SHIFT = 1 << 9;
        const ALPHA_MASK = 1 << 10;
        const COLORMAP = 1 << 11;
    }
}

impl XcbPictFormat {
    fn info(self) -> (xcb_render_pictforminfo_t, XcbPictFormatFeatures) {
        match self {
            XcbPictFormat::Argb32 => (
                xcb_render_pictforminfo_t {
                    type_: XCB_RENDER_PICT_TYPE_DIRECT as _,
                    depth: 32,
                    direct: xcb_render_directformat_t {
                        alpha_shift: 24,
                        alpha_mask: 0xff,
                        red_shift: 16,
                        red_mask: 0xff,
                        green_shift: 8,
                        green_mask: 0xff,
                        blue_shift: 0,
                        blue_mask: 0xff,
                    },
                    ..Default::default()
                },
                XcbPictFormatFeatures::TYPE
                    | XcbPictFormatFeatures::DEPTH
                    | XcbPictFormatFeatures::RED_SHIFT
                    | XcbPictFormatFeatures::RED_MASK
                    | XcbPictFormatFeatures::GREEN_SHIFT
                    | XcbPictFormatFeatures::GREEN_MASK
                    | XcbPictFormatFeatures::BLUE_SHIFT
                    | XcbPictFormatFeatures::BLUE_MASK
                    | XcbPictFormatFeatures::ALPHA_SHIFT
                    | XcbPictFormatFeatures::ALPHA_MASK,
            ),
            XcbPictFormat::Rgb24 => (
                xcb_render_pictforminfo_t {
                    type_: XCB_RENDER_PICT_TYPE_DIRECT as _,
                    depth: 24,
                    direct: xcb_render_directformat_t {
                        red_shift: 16,
                        red_mask: 0xff,
                        green_shift: 8,
                        green_mask: 0xff,
                        blue_shift: 0,
                        blue_mask: 0xff,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                XcbPictFormatFeatures::TYPE
                    | XcbPictFormatFeatures::DEPTH
                    | XcbPictFormatFeatures::RED_SHIFT
                    | XcbPictFormatFeatures::RED_MASK
                    | XcbPictFormatFeatures::GREEN_SHIFT
                    | XcbPictFormatFeatures::GREEN_MASK
                    | XcbPictFormatFeatures::BLUE_SHIFT
                    | XcbPictFormatFeatures::BLUE_MASK
                    | XcbPictFormatFeatures::ALPHA_MASK,
            ),
            XcbPictFormat::A8 | XcbPictFormat::A4 | XcbPictFormat::A1 => {
                let (depth, alpha_mask) = match self {
                    XcbPictFormat::A8 => (8, 0xff),
                    XcbPictFormat::A4 => (4, 0x0f),
                    XcbPictFormat::A1 => (1, 0x01),
                    _ => unreachable!(),
                };
                (
                    xcb_render_pictforminfo_t {
                        type_: XCB_RENDER_PICT_TYPE_DIRECT as _,
                        depth,
                        direct: xcb_render_directformat_t {
                            alpha_shift: 0,
                            alpha_mask,
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    XcbPictFormatFeatures::TYPE
                        | XcbPictFormatFeatures::DEPTH
                        | XcbPictFormatFeatures::RED_MASK
                        | XcbPictFormatFeatures::GREEN_MASK
                        | XcbPictFormatFeatures::BLUE_MASK
                        | XcbPictFormatFeatures::ALPHA_SHIFT
                        | XcbPictFormatFeatures::ALPHA_MASK,
                )
            }
        }
    }
}

pub unsafe fn find_format(
    render: &XcbRender,
    formats: *const xcb_render_query_pict_formats_reply_t,
    format: &xcb_render_pictforminfo_t,
    features: XcbPictFormatFeatures,
) -> Option<xcb_render_pictforminfo_t> {
    let mut iter = render.xcb_render_query_pict_formats_formats_iterator(formats);
    while iter.rem > 0 {
        'inner: loop {
            let actual = &*iter.data;
            macro_rules! compare {
                ($($field:ident).+, $feature:ident) => {
                    if features.contains(XcbPictFormatFeatures::$feature) {
                        if format.$($field).* != actual.$($field).* {
                            break 'inner;
                        }
                    }
                }
            }
            compare!(id, ID);
            compare!(type_, TYPE);
            compare!(depth, DEPTH);
            compare!(direct.red_shift, RED_SHIFT);
            compare!(direct.red_mask, RED_MASK);
            compare!(direct.blue_shift, BLUE_SHIFT);
            compare!(direct.blue_mask, BLUE_MASK);
            compare!(direct.green_shift, GREEN_SHIFT);
            compare!(direct.green_mask, GREEN_MASK);
            compare!(direct.alpha_shift, ALPHA_SHIFT);
            compare!(direct.alpha_mask, ALPHA_MASK);
            compare!(colormap, COLORMAP);
            return Some(*actual);
        }
        render.xcb_render_pictforminfo_next(&mut iter);
    }
    None
}

pub unsafe fn find_standard_format(
    render: &XcbRender,
    formats: *const xcb_render_query_pict_formats_reply_t,
    format: XcbPictFormat,
) -> Option<xcb_render_pictforminfo_t> {
    let (format, features) = format.info();
    find_format(render, formats, &format, features)
}
