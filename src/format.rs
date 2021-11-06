use std::mem;

/// A type that can be sent via client messages or be stored in properties.
///
/// # Safety
///
/// This can be implemented for any type that admits every possible bit pattern.
pub unsafe trait XcbDataType: Copy + std::fmt::Debug + Sized {
    /// The number of bits in this type.
    ///
    /// This must not be implemented manually.
    const XCB_BITS: u8 = mem::size_of::<Self>() as u8 * 8;
}

macro_rules! imp {
    ($ty:ty) => {
        unsafe impl XcbDataType for $ty {}
    };
}

imp!(u8);
imp!(u16);
imp!(u32);
imp!(i8);
imp!(i16);
imp!(i32);
