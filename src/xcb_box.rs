use std::fmt::{Debug, Formatter};
use std::ops::{Deref, DerefMut};
use std::ptr;

pub struct XcbBox<T> {
    t: ptr::NonNull<T>,
}

impl<T> XcbBox<T> {
    pub unsafe fn new(t: *mut T) -> Self {
        Self {
            t: ptr::NonNull::new_unchecked(t),
        }
    }
}

impl<T> Deref for XcbBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.t.as_ptr() }
    }
}

impl<T> DerefMut for XcbBox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.t.as_ptr() }
    }
}

impl<T> Drop for XcbBox<T> {
    fn drop(&mut self) {
        unsafe {
            libc::free(self.t.as_ptr() as _);
        }
    }
}

impl<T: Debug> Debug for XcbBox<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&**self, f)
    }
}
