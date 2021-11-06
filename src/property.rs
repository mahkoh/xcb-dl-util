use crate::error::{XcbError, XcbErrorParser};
use crate::format::XcbDataType;
use std::{ptr, slice};
use thiserror::Error;
use xcb_dl::ffi::*;
use xcb_dl::*;

#[derive(Clone, Debug, Error)]
pub enum XcbGetPropertyError {
    #[error("Invalid property type (expected: {expected}, actual: {actual})")]
    InvalidPropertyType {
        expected: xcb_atom_t,
        actual: xcb_atom_t,
    },
    #[error("Invalid property format (expected: {expected}, actual: {actual})")]
    InvalidPropertyFormat { expected: u8, actual: u8 },
    #[error("The property is not set")]
    Unset,
    #[error("xcb error: {0}")]
    Xcb(#[from] XcbError),
}

pub unsafe fn get_property<T: XcbDataType>(
    xcb: &Xcb,
    errors: &XcbErrorParser,
    window: xcb_window_t,
    property: xcb_atom_t,
    type_: xcb_atom_t,
    delete: bool,
    step: u32,
) -> Result<Vec<T>, XcbGetPropertyError> {
    let mut buf = vec![];
    get_property_in(xcb, errors, window, property, type_, delete, step, &mut buf)?;
    Ok(buf)
}

pub unsafe fn get_property_in<T: XcbDataType>(
    xcb: &Xcb,
    errors: &XcbErrorParser,
    window: xcb_window_t,
    property: xcb_atom_t,
    type_: xcb_atom_t,
    delete: bool,
    step: u32,
    buf: &mut Vec<T>,
) -> Result<(), XcbGetPropertyError> {
    let mut offset = 0;
    loop {
        let mut err = ptr::null_mut();
        let res = xcb.xcb_get_property_reply(
            errors.c,
            xcb.xcb_get_property(
                errors.c,
                delete as u8,
                window,
                property,
                type_,
                offset,
                step,
            ),
            &mut err,
        );
        let res = errors.check(xcb, res, err)?;
        if res.type_ != type_ {
            if res.type_ == 0 {
                return Err(XcbGetPropertyError::Unset);
            }
            return Err(XcbGetPropertyError::InvalidPropertyType {
                expected: type_,
                actual: res.type_,
            });
        }
        if res.format != T::XCB_BITS {
            return Err(XcbGetPropertyError::InvalidPropertyFormat {
                expected: T::XCB_BITS,
                actual: res.format,
            });
        }
        let value = xcb.xcb_get_property_value(&*res);
        buf.extend_from_slice(slice::from_raw_parts(
            value as *const T,
            res.value_len as usize,
        ));
        if res.bytes_after == 0 {
            break;
        }
        offset += step;
    }
    Ok(())
}
