use crate::error::{XcbError, XcbErrorParser};
use crate::render::{find_standard_format, XcbPictFormat};
use bstr::{BStr, BString, ByteSlice, ByteVec};
use byteorder::{LittleEndian, ReadBytesExt};
use isnt::std_1::primitive::IsntSliceExt;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt::{Debug, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::{env, io, ptr, slice, str};
use thiserror::Error;
use xcb_dl::ffi::*;
use xcb_dl::Xcb;
use xcb_dl::XcbRender;

const XCURSOR_MAGIC: u32 = 0x72756358;
const XCURSOR_IMAGE_TYPE: u32 = 0xfffd0002;
const XCURSOR_PATH_DEFAULT: &[u8] =
    b"~/.icons:/usr/share/icons:/usr/share/pixmaps:/usr/X11R6/lib/X11/icons";
const XCURSOR_PATH: &str = "XCURSOR_PATH";
const HOME: &str = "HOME";
const CURSOR_FONT: &str = "cursor";
const DEPTH: u8 = 32;

const HEADER_SIZE: u32 = 16;

#[derive(Debug)]
pub struct XcbCursorContext {
    c: *mut xcb_connection_t,
    core_map: HashMap<&'static BStr, u16>,
    errors: XcbErrorParser,
    theme: Option<BString>,
    size: u32,
    font_id: xcb_font_t,
    cursor_paths: Vec<BString>,
    config: Option<RenderConfig>,
    root: xcb_window_t,
    visual: xcb_visualid_t,
}

impl XcbCursorContext {
    pub unsafe fn new(xcb: &Xcb, render: &XcbRender, c: *mut xcb_connection_t) -> Self {
        let errors = XcbErrorParser::new(xcb, c);
        let (theme, size, root, visual) = resource_values(xcb, &errors, c);
        let font_id = xcb.xcb_generate_id(c);
        xcb.xcb_open_font(
            c,
            font_id,
            CURSOR_FONT.len() as _,
            CURSOR_FONT.as_ptr() as *const _,
        );
        Self {
            c,
            core_map: core_map(),
            theme,
            size,
            font_id,
            cursor_paths: find_cursor_paths(),
            config: find_render_config(xcb, render, &errors, c),
            errors,
            root,
            visual,
        }
    }

    pub unsafe fn create_cursor(
        &self,
        xcb: &Xcb,
        render: &XcbRender,
        mut images: &[XcbCursorImage],
    ) -> Result<xcb_cursor_t, XcbCursorError> {
        if images.is_empty() {
            return Err(XcbCursorError::EmptyXcursorFile);
        }
        let config = match self.config {
            Some(c) => c,
            None => return Err(XcbCursorError::ImageCursorNotSupported),
        };
        if !config.animated {
            images = &images[..1];
        }
        let pic = xcb.xcb_generate_id(self.c);
        let mut pixmap = XCB_NONE;
        let mut gc = XCB_NONE;
        let mut prev_width = !0;
        let mut prev_height = !0;

        let mut elements = vec![];
        let mut requests = vec![];

        macro_rules! d {
            ($e:expr) => {
                xcb.xcb_discard_reply(self.c, $e.sequence);
            };
        }

        for image in images {
            if pixmap == XCB_NONE || prev_width != image.width || prev_height != image.height {
                if pixmap == XCB_NONE {
                    pixmap = xcb.xcb_generate_id(self.c);
                    gc = xcb.xcb_generate_id(self.c);
                } else {
                    d!(xcb.xcb_free_pixmap_checked(self.c, pixmap));
                    d!(xcb.xcb_free_gc_checked(self.c, gc));
                }
                d!(xcb.xcb_create_pixmap_checked(
                    self.c,
                    DEPTH,
                    pixmap,
                    self.root,
                    image.width,
                    image.height
                ));
                d!(xcb.xcb_create_gc_checked(self.c, gc, pixmap, 0, ptr::null()));
                prev_width = image.width;
                prev_height = image.height;
            }
            let cookie = xcb.xcb_put_image_checked(
                self.c,
                XCB_IMAGE_FORMAT_Z_PIXMAP as _,
                pixmap,
                gc,
                image.width,
                image.height,
                0,
                0,
                0,
                DEPTH,
                (4 * image.pixels.len()) as _,
                image.pixels.as_ptr() as _,
            );
            requests.push(cookie);
            d!(render.xcb_render_create_picture_checked(
                self.c,
                pic,
                pixmap,
                config.format,
                0,
                ptr::null()
            ));
            let cursor = xcb.xcb_generate_id(self.c);
            let cookie = render
                .xcb_render_create_cursor_checked(self.c, cursor, pic, image.xhot, image.yhot);
            requests.push(cookie);
            d!(render.xcb_render_free_picture_checked(self.c, pic));
            elements.push(xcb_render_animcursorelt_t {
                cursor,
                delay: image.delay,
            });
        }

        d!(xcb.xcb_free_pixmap_checked(self.c, pixmap));
        d!(xcb.xcb_free_gc_checked(self.c, gc));

        let cursor = if elements.len() == 1 {
            elements[0].cursor
        } else {
            let cursor = xcb.xcb_generate_id(self.c);
            let cookie = render.xcb_render_create_anim_cursor_checked(
                self.c,
                cursor,
                elements.len() as _,
                elements.as_ptr(),
            );
            requests.push(cookie);
            for element in elements {
                d!(xcb.xcb_free_cursor_checked(self.c, element.cursor));
            }
            cursor
        };

        let mut err = Ok(());
        for request in requests {
            let e = xcb.xcb_request_check(self.c, request);
            err = err.and(self.errors.check_err(e));
        }
        if let Err(err) = err {
            d!(xcb.xcb_free_cursor_checked(self.c, cursor));
            Err(err.into())
        } else {
            Ok(cursor)
        }
    }

    pub unsafe fn load_cursor(
        &self,
        xcb: &Xcb,
        render: &XcbRender,
        config: &XcbLoadCursorConfig,
    ) -> Result<xcb_cursor_t, XcbCursorError> {
        let name = config.name;
        let mut file = None;
        if self.config.is_some() {
            let theme = config
                .theme
                .map(|t| t.as_bytes())
                .or(self.theme.as_ref().map(|t| t.as_bytes()));
            if let Some(theme) = theme {
                file = self.open_cursor_file(theme, name);
            }
            if file.is_none() {
                file = self.open_cursor_file(b"default", name);
            }
        }
        let file = match file {
            Some(f) => f,
            _ => {
                if let Some(id) = self.core_map.get(name.as_bytes().as_bstr()) {
                    OpenedCursorFile::CoreId(*id)
                } else {
                    return Err(XcbCursorError::NotFound);
                }
            }
        };
        let file = match file {
            OpenedCursorFile::File(f) => f,
            OpenedCursorFile::CoreId(id) => {
                let cursor_id = xcb.xcb_generate_id(self.c);
                let cookie = xcb.xcb_create_glyph_cursor_checked(
                    self.c,
                    cursor_id,
                    self.font_id,
                    self.font_id,
                    id,
                    id + 1,
                    0,
                    0,
                    0,
                    !0,
                    !0,
                    !0,
                );
                if self
                    .errors
                    .check_err(xcb.xcb_request_check(self.c, cookie))
                    .is_err()
                {
                    return Err(XcbCursorError::NotFound);
                } else {
                    return Ok(cursor_id);
                }
            }
        };
        let mut file = BufReader::new(file);
        let size = config.size.unwrap_or(self.size);
        let images = parser_cursor_file(&mut file, size)?;
        self.create_cursor(xcb, render, &images)
    }

    fn open_cursor_file(&self, theme: &[u8], name: &str) -> Option<OpenedCursorFile> {
        if theme == b"core" {
            if let Some(id) = self.core_map.get(name.as_bytes().as_bstr()) {
                return Some(OpenedCursorFile::CoreId(*id));
            }
        }
        if self.cursor_paths.is_empty() {
            return None;
        }
        let mut parents = None;
        for cursor_path in &self.cursor_paths {
            let mut theme_dir = cursor_path.clone();
            theme_dir.push(b'/');
            theme_dir.extend_from_slice(theme);
            let mut cursor_file = theme_dir.clone();
            cursor_file.extend_from_slice(b"/cursors/");
            cursor_file.extend_from_slice(name.as_bytes());
            if let Ok(f) = File::open(cursor_file.to_os_str().unwrap()) {
                return Some(OpenedCursorFile::File(f));
            }
            if parents.is_none() {
                let mut index_file = theme_dir.clone();
                index_file.extend_from_slice(b"/index.theme");
                parents = find_parent_themes(&index_file);
            }
        }
        if let Some(parents) = parents {
            for parent in parents {
                // NOTE: If there is a cycle, this will recurse until it overflows the stack.
                if let Some(file) = self.open_cursor_file(&parent, name) {
                    return Some(file);
                }
            }
        }
        None
    }
}

#[derive(Clone, Debug, Default)]
pub struct XcbLoadCursorConfig<'a> {
    pub name: &'a str,
    pub theme: Option<&'a str>,
    pub size: Option<u32>,
}

#[derive(Debug)]
enum OpenedCursorFile {
    File(File),
    CoreId(u16),
}

#[derive(Copy, Clone, Debug)]
struct RenderConfig {
    format: xcb_render_pictformat_t,
    animated: bool,
}

#[test]
fn test() {
    use simple_logger::SimpleLogger;
    unsafe {
        SimpleLogger::new().init().unwrap();
        let xcb = Xcb::load().unwrap();
        let render = XcbRender::load().unwrap();
        let c = xcb.xcb_connect(ptr::null(), ptr::null_mut());
        let ctx = XcbCursorContext::new(&xcb, &render, c);
        let window_id = xcb.xcb_generate_id(c);
        xcb.xcb_create_window(
            c,
            24,
            window_id,
            ctx.root,
            0,
            0,
            100,
            100,
            0,
            0,
            ctx.visual,
            0,
            ptr::null(),
        );
        xcb.xcb_map_window(c, window_id);
        for cursor in ctx.core_map.keys() {
            let config = XcbLoadCursorConfig {
                name: cursor.to_str_unchecked(),
                theme: Some("breeze_cursors"),
                size: Some(100),
                ..Default::default()
            };
            let res = ctx.load_cursor(&xcb, &render, &config).unwrap();
            xcb.xcb_change_window_attributes(
                c,
                window_id,
                XCB_CW_CURSOR,
                &res as *const _ as *const _,
            );
            xcb.xcb_flush(c);
            std::thread::sleep_ms(2000);
        }
    }
}

fn find_cursor_paths() -> Vec<BString> {
    let home = env::var_os(HOME).map(|h| Vec::from_os_string(h).unwrap());
    let cursor_paths = env::var_os(XCURSOR_PATH);
    let cursor_paths = cursor_paths
        .as_ref()
        .map(|c| <[u8]>::from_os_str(c).unwrap())
        .unwrap_or(XCURSOR_PATH_DEFAULT);
    let mut paths = vec![];
    for path in <[u8]>::split(cursor_paths, |b| *b == b':') {
        if path.first() == Some(&b'~') {
            if let Some(home) = home.as_ref() {
                let mut full_path = home.clone();
                full_path.extend_from_slice(&path[1..]);
                paths.push(full_path.into());
            }
        } else {
            paths.push(path.as_bstr().to_owned());
        }
    }
    paths
}

unsafe fn find_render_config(
    xcb: &Xcb,
    render: &XcbRender,
    errors: &XcbErrorParser,
    c: *mut xcb_connection_t,
) -> Option<RenderConfig> {
    let ext = xcb.xcb_get_extension_data(c, render.xcb_render_id());
    if ext.is_null() || (*ext).present == 0 {
        return None;
    }
    let mut err = ptr::null_mut();
    let reply = render.xcb_render_query_version_reply(
        c,
        render.xcb_render_query_version(c, 0, 11),
        &mut err,
    );
    let version = match errors.check(xcb, reply, err) {
        Ok(v) => v,
        Err(e) => {
            log::error!("Could not query xcb render version: {}", e);
            return None;
        }
    };
    let version = (version.major_version, version.minor_version);
    let animated = if version >= (0, 8) {
        true
    } else if version >= (0, 5) {
        log::warn!("Render extension is too old to support animated cursors");
        false
    } else {
        log::warn!("Render extension does not support cursors");
        return None;
    };
    let formats = render.xcb_render_query_pict_formats_reply(
        c,
        render.xcb_render_query_pict_formats(c),
        &mut err,
    );
    let formats = match errors.check(xcb, formats, err) {
        Ok(v) => v,
        Err(e) => {
            log::error!("Could not query picture formats: {}", e);
            return None;
        }
    };
    let format = match find_standard_format(render, &*formats, XcbPictFormat::Argb32) {
        Some(f) => f,
        None => {
            log::warn!("Render extension does not support RGBA images");
            return None;
        }
    };
    Some(RenderConfig {
        format: format.id,
        animated,
    })
}

fn find_parent_themes(path: &[u8]) -> Option<Vec<BString>> {
    // NOTE: The files we're reading here are really INI files with a hierarchy. This
    // algorithm treats it as a flat list and is inherited from libxcursor.
    let file = match File::open(path.to_os_str().unwrap()) {
        Ok(f) => f,
        _ => return None,
    };
    let mut buf_reader = BufReader::new(file);
    let mut buf = vec![];
    loop {
        buf.clear();
        match buf_reader.read_until(b'\n', &mut buf) {
            Ok(n) if n > 0 => {}
            _ => return None,
        }
        let mut suffix = match buf.strip_prefix(b"Inherits") {
            Some(s) => s,
            _ => continue,
        };
        while suffix.first() == Some(&b' ') {
            suffix = &suffix[1..];
        }
        if suffix.first() != Some(&b'=') {
            continue;
        }
        suffix = &suffix[1..];
        let parents = suffix
            .split(|b| matches!(*b, b' ' | b'\t' | b'\n' | b';' | b','))
            .filter(|v| v.is_not_empty())
            .map(|v| v.as_bstr().to_owned())
            .collect();
        return Some(parents);
    }
}

unsafe fn resource_values(
    xcb: &Xcb,
    errors: &XcbErrorParser,
    c: *mut xcb_connection_t,
) -> (Option<BString>, u32, xcb_window_t, xcb_visualid_t) {
    let mut res = (None, 0, XCB_NONE, XCB_NONE);

    let setup = xcb.xcb_get_setup(c);
    let screens = xcb.xcb_setup_roots_iterator(setup);
    if screens.rem == 0 {
        log::warn!("X server has no screens");
        return res;
    }
    let screen = &*screens.data;

    let dim = screen.height_in_pixels.min(screen.width_in_pixels);
    res.1 = dim as u32 / 48;
    res.2 = screen.root;
    res.3 = screen.root_visual;

    let cookie = xcb.xcb_get_property(
        c,
        0,
        screen.root,
        XCB_ATOM_RESOURCE_MANAGER,
        XCB_ATOM_STRING,
        0,
        1024 * 1024 * 256,
    );
    let mut err = ptr::null_mut();
    let reply = xcb.xcb_get_property_reply(c, cookie, &mut err);
    let reply = match errors.check(xcb, reply, err) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("Could not read the resource manager property: {}", e);
            return res;
        }
    };
    if reply.type_ == 0 {
        // Property not set.
        return res;
    }
    if reply.type_ != XCB_ATOM_STRING || reply.format != 8 {
        log::warn!("Resource manager property has an invalid format");
        return res;
    }
    let value = slice::from_raw_parts(
        xcb.xcb_get_property_value(&*reply) as *mut u8,
        reply.value_len as _,
    );

    let mut xcursor_size = None;
    let mut xft_dpi = None;
    // https://github.com/intellij-rust/intellij-rust/issues/8021
    for line in <[u8]>::split(value, |b| *b == b'\n') {
        let (name, value) = match line.iter().position(|b| *b == b':') {
            Some(v) => line.split_at(v),
            _ => continue,
        };
        let value = &value[1..];
        match name {
            b"Xcursor.theme" => res.0 = Some(posix_trim_start(value).as_bstr().to_owned()),
            b"Xcursor.size" => xcursor_size = parse_u32(value),
            b"Xft.dpi" => xft_dpi = parse_u32(value),
            _ => {}
        }
    }

    if let Some(xcursor_size) = xcursor_size {
        res.1 = xcursor_size;
    } else if let Some(xft_dpi) = xft_dpi {
        if xft_dpi > 0 {
            res.1 = xft_dpi * 16 / 72;
        }
    }

    res
}

fn core_map() -> HashMap<&'static BStr, u16> {
    let mut map = HashMap::new();
    map.insert(b"X_cursor".as_bstr(), 0);
    map.insert(b"arrow".as_bstr(), 1);
    map.insert(b"based_arrow_down".as_bstr(), 2);
    map.insert(b"based_arrow_up".as_bstr(), 3);
    map.insert(b"boat".as_bstr(), 4);
    map.insert(b"bogosity".as_bstr(), 5);
    map.insert(b"bottom_left_corner".as_bstr(), 6);
    map.insert(b"bottom_right_corner".as_bstr(), 7);
    map.insert(b"bottom_side".as_bstr(), 8);
    map.insert(b"bottom_tee".as_bstr(), 9);
    map.insert(b"box_spiral".as_bstr(), 10);
    map.insert(b"center_ptr".as_bstr(), 11);
    map.insert(b"circle".as_bstr(), 12);
    map.insert(b"clock".as_bstr(), 13);
    map.insert(b"coffee_mug".as_bstr(), 14);
    map.insert(b"cross".as_bstr(), 15);
    map.insert(b"cross_reverse".as_bstr(), 16);
    map.insert(b"crosshair".as_bstr(), 17);
    map.insert(b"diamond_cross".as_bstr(), 18);
    map.insert(b"dot".as_bstr(), 19);
    map.insert(b"dotbox".as_bstr(), 20);
    map.insert(b"double_arrow".as_bstr(), 21);
    map.insert(b"draft_large".as_bstr(), 22);
    map.insert(b"draft_small".as_bstr(), 23);
    map.insert(b"draped_box".as_bstr(), 24);
    map.insert(b"exchange".as_bstr(), 25);
    map.insert(b"fleur".as_bstr(), 26);
    map.insert(b"gobbler".as_bstr(), 27);
    map.insert(b"gumby".as_bstr(), 28);
    map.insert(b"hand1".as_bstr(), 29);
    map.insert(b"hand2".as_bstr(), 30);
    map.insert(b"heart".as_bstr(), 31);
    map.insert(b"icon".as_bstr(), 32);
    map.insert(b"iron_cross".as_bstr(), 33);
    map.insert(b"left_ptr".as_bstr(), 34);
    map.insert(b"left_side".as_bstr(), 35);
    map.insert(b"left_tee".as_bstr(), 36);
    map.insert(b"leftbutton".as_bstr(), 37);
    map.insert(b"ll_angle".as_bstr(), 38);
    map.insert(b"lr_angle".as_bstr(), 39);
    map.insert(b"man".as_bstr(), 40);
    map.insert(b"middlebutton".as_bstr(), 41);
    map.insert(b"mouse".as_bstr(), 42);
    map.insert(b"pencil".as_bstr(), 43);
    map.insert(b"pirate".as_bstr(), 44);
    map.insert(b"plus".as_bstr(), 45);
    map.insert(b"question_arrow".as_bstr(), 46);
    map.insert(b"right_ptr".as_bstr(), 47);
    map.insert(b"right_side".as_bstr(), 48);
    map.insert(b"right_tee".as_bstr(), 49);
    map.insert(b"rightbutton".as_bstr(), 50);
    map.insert(b"rtl_logo".as_bstr(), 51);
    map.insert(b"sailboat".as_bstr(), 52);
    map.insert(b"sb_down_arrow".as_bstr(), 53);
    map.insert(b"sb_h_double_arrow".as_bstr(), 54);
    map.insert(b"sb_left_arrow".as_bstr(), 55);
    map.insert(b"sb_right_arrow".as_bstr(), 56);
    map.insert(b"sb_up_arrow".as_bstr(), 57);
    map.insert(b"sb_v_double_arrow".as_bstr(), 58);
    map.insert(b"shuttle".as_bstr(), 59);
    map.insert(b"sizing".as_bstr(), 60);
    map.insert(b"spider".as_bstr(), 61);
    map.insert(b"spraycan".as_bstr(), 62);
    map.insert(b"star".as_bstr(), 63);
    map.insert(b"target".as_bstr(), 64);
    map.insert(b"tcross".as_bstr(), 65);
    map.insert(b"top_left_arrow".as_bstr(), 66);
    map.insert(b"top_left_corner".as_bstr(), 67);
    map.insert(b"top_right_corner".as_bstr(), 68);
    map.insert(b"top_side".as_bstr(), 69);
    map.insert(b"top_tee".as_bstr(), 70);
    map.insert(b"trek".as_bstr(), 71);
    map.insert(b"ul_angle".as_bstr(), 72);
    map.insert(b"umbrella".as_bstr(), 73);
    map.insert(b"ur_angle".as_bstr(), 74);
    map.insert(b"watch".as_bstr(), 75);
    map.insert(b"xterm".as_bstr(), 76);
    map
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum XcbCursorError {
    #[error("An IO error occurred: {0}")]
    Io(#[from] io::Error),
    #[error("An IO error occurred: {0}")]
    Xcb(#[from] XcbError),
    #[error("The file is not an Xcursor file")]
    NotAnXcursorFile,
    #[error("The Xcursor file contains more than 0x10000 images")]
    OversizedXcursorFile,
    #[error("The Xcursor file is empty")]
    EmptyXcursorFile,
    #[error("The Xcursor file is corrupt")]
    CorruptXcursorFile,
    #[error("The requested cursor could not be found")]
    NotFound,
    #[error("Cursors from images are not supported")]
    ImageCursorNotSupported,
}

#[derive(Default, Clone)]
pub struct XcbCursorImage {
    pub width: u16,
    pub height: u16,
    pub xhot: u16,
    pub yhot: u16,
    pub delay: u32,
    pub pixels: Vec<u32>,
}

impl Debug for XcbCursorImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XcbCursorImage")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("xhot", &self.xhot)
            .field("yhot", &self.yhot)
            .field("delay", &self.delay)
            .finish_non_exhaustive()
    }
}

fn parser_cursor_file<R: BufRead + Seek>(
    r: &mut R,
    target: u32,
) -> Result<Vec<XcbCursorImage>, XcbCursorError> {
    let [magic, header] = read_u32_n(r)?;
    if magic != XCURSOR_MAGIC || header < HEADER_SIZE {
        return Err(XcbCursorError::NotAnXcursorFile);
    }
    let [_version, ntoc] = read_u32_n(r)?;
    r.seek(SeekFrom::Current((HEADER_SIZE - header) as i64))?;
    if ntoc > 0x10000 {
        return Err(XcbCursorError::OversizedXcursorFile);
    }
    let mut images_positions = vec![];
    let mut best_fit = i64::MAX;
    for _ in 0..ntoc {
        let [type_, size, position] = read_u32_n(r)?;
        if type_ != XCURSOR_IMAGE_TYPE {
            continue;
        }
        let fit = (size as i64 - target as i64).abs();
        if fit < best_fit {
            best_fit = fit;
            images_positions.clear();
        }
        if fit == best_fit {
            images_positions.push(position);
        }
    }
    let mut images = Vec::with_capacity(images_positions.len());
    for position in images_positions {
        r.seek(SeekFrom::Start(position as u64))?;
        let [_chunk_header, _type_, _size, _version, width, height, xhot, yhot, delay] =
            read_u32_n(r)?;
        let [width, height, xhot, yhot] = u32_to_u16([width, height, xhot, yhot])?;
        let mut image = XcbCursorImage {
            width,
            height,
            xhot,
            yhot,
            delay,
            pixels: vec![],
        };
        let num_pixels = width as usize * height as usize;
        unsafe {
            image.pixels.reserve_exact(num_pixels as usize);
            image.pixels.set_len(num_pixels as usize);
            r.read_u32_into::<LittleEndian>(&mut image.pixels)?;
        }
        images.push(image);
    }
    Ok(images)
}

fn read_u32_n<R: BufRead, const N: usize>(r: &mut R) -> Result<[u32; N], io::Error> {
    let mut res = [0; N];
    r.read_u32_into::<LittleEndian>(&mut res)?;
    Ok(res)
}

fn posix_trim_start(b: &[u8]) -> &[u8] {
    b.trim_start_with(|b| matches!(b, ' ' | '\t'))
}

fn parse_u32(b: &[u8]) -> Option<u32> {
    str::from_utf8(b)
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
}

fn u32_to_u16<const N: usize>(n: [u32; N]) -> Result<[u16; N], XcbCursorError> {
    let mut res = [0; N];
    for i in 0..N {
        res[i] = n[i]
            .try_into()
            .map_err(|_| XcbCursorError::CorruptXcursorFile)?;
    }
    Ok(res)
}
