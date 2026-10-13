//! Handles typed by the object they name, and handle values typed by the table they belong to.
//!
//! A `BorrowedHandle` or an `isize` would let a thread handle reach a call for processes,
//! or a value of the creator's table join a list of the inheritor's values; these types make both a compile error.

use std::fmt;
use std::marker::PhantomData;
use std::os::windows::io::{AsHandle, AsRawHandle, BorrowedHandle, OwnedHandle, RawHandle};

/// A process.
pub(crate) enum Process {}
/// A thread.
pub(crate) enum Thread {}
/// A Job object.
pub(crate) enum JobObject {}
/// One end of an anonymous pipe.
pub(crate) enum Pipe {}
/// A file, device, pipe, or console handle that a child receives as a standard stream.
pub(crate) enum Stream {}
/// A handle the caller supplied, whose kind the crate doesn't know.
pub(crate) enum Foreign {}

/// An owned handle, in the current process's table, to an object of kind `K`.
pub(crate) struct Owned<K> {
    handle: OwnedHandle,
    kind: PhantomData<K>,
}

impl<K> Owned<K> {
    /// Adopts `handle` as a handle to an object of kind `K`.
    ///
    /// The caller has created the object with an API of that kind, or checked the kind with one.
    pub(crate) const fn adopt(handle: OwnedHandle) -> Self {
        Self {
            handle,
            kind: PhantomData,
        }
    }

    /// Borrows the handle with its kind.
    pub(crate) fn borrow(&self) -> Borrowed<'_, K> {
        Borrowed {
            handle: self.handle.as_handle(),
            kind: PhantomData,
        }
    }

    /// Gives up the kind, for a caller that receives a plain handle.
    pub(crate) fn into_inner(self) -> OwnedHandle {
        self.handle
    }
}

impl<K> AsHandle for Owned<K> {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.handle.as_handle()
    }
}

impl<K> AsRawHandle for Owned<K> {
    fn as_raw_handle(&self) -> RawHandle {
        self.handle.as_raw_handle()
    }
}

impl<K> fmt::Debug for Owned<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.handle.fmt(formatter)
    }
}

/// A borrowed handle, in the current process's table, to an object of kind `K`.
pub(crate) struct Borrowed<'a, K> {
    handle: BorrowedHandle<'a>,
    kind: PhantomData<K>,
}

impl<'a> Borrowed<'a, Foreign> {
    /// Borrows a handle the caller supplied.
    pub(crate) fn foreign<T: AsHandle + ?Sized>(source: &'a T) -> Self {
        Self::adopt(source.as_handle())
    }
}

impl<'a, K> Borrowed<'a, K> {
    /// Borrows `handle` as a handle to an object of kind `K`, which the caller has established.
    pub(crate) const fn adopt(handle: BorrowedHandle<'a>) -> Self {
        Self {
            handle,
            kind: PhantomData,
        }
    }

    /// Returns the plain borrowed handle.
    pub(crate) const fn untyped(self) -> BorrowedHandle<'a> {
        self.handle
    }
}

impl<K> Clone for Borrowed<'_, K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Borrowed<'_, K> {}

/// The table of the process that creates the child.
pub(crate) enum Creator {}
/// The table of the process the child inherits from: the creator, or the alternate parent.
pub(crate) enum Inheritor {}

/// A handle value valid in the handle table `T`.
///
/// Its layout is an `isize`'s, so a slice of values is the array an attribute list reads.
#[repr(transparent)]
pub(crate) struct Value<T> {
    raw: isize,
    table: PhantomData<T>,
}

impl<T> Value<T> {
    pub(crate) const fn as_raw(self) -> isize {
        self.raw
    }
}

impl Value<Creator> {
    /// The value of a handle in the current process's table.
    pub(crate) fn of(handle: BorrowedHandle<'_>) -> Self {
        Self {
            raw: crate::domain::conversion_rules::handle_value(handle.as_raw_handle()),
            table: PhantomData,
        }
    }

    /// The same value in the inheritor's table, which is the creator's when no alternate parent is named.
    pub(crate) const fn inherited(self) -> Value<Inheritor> {
        Value {
            raw: self.raw,
            table: PhantomData,
        }
    }
}

impl Value<Inheritor> {
    /// The value of a standard stream that a child doesn't receive.
    pub(crate) const INVALID: Self = Self {
        raw: crate::sys::INVALID_RAW_HANDLE,
        table: PhantomData,
    };

    /// The value of a duplicate in the alternate parent's table.
    pub(crate) const fn in_parent(raw: isize) -> Self {
        Self {
            raw,
            table: PhantomData,
        }
    }
}

impl<T> Clone for Value<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Value<T> {}

impl<T> PartialEq for Value<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<T> Eq for Value<T> {}

impl<T> fmt::Display for Value<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.raw.fmt(formatter)
    }
}

impl<T> fmt::Debug for Value<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.raw.fmt(formatter)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::mem::size_of;

    use super::*;

    #[test]
    fn values_keep_the_layout_and_text_of_their_number() {
        assert_eq!(size_of::<Value<Creator>>(), size_of::<isize>());
        let file = File::open("NUL").unwrap();
        let value = Value::<Creator>::of(file.as_handle());
        assert_eq!(value.to_string(), value.as_raw().to_string());
        assert_eq!(format!("{value:?}"), value.as_raw().to_string());
        assert_eq!(value.inherited().as_raw(), value.as_raw());
        assert_eq!(Value::<Inheritor>::in_parent(42).as_raw(), 42);
        assert_eq!(Value::<Inheritor>::INVALID.as_raw(), -1);
        let copy = value;
        assert_eq!(copy, value);
        assert_ne!(Value::<Inheritor>::in_parent(1), Value::in_parent(2));

        let owned = Owned::<Stream>::adopt(OwnedHandle::from(file));
        assert_eq!(
            owned.borrow().untyped().as_raw_handle(),
            owned.as_raw_handle()
        );
        assert!(format!("{owned:?}").contains("OwnedHandle"));
        let raw = owned.as_raw_handle();
        assert_eq!(owned.into_inner().as_raw_handle(), raw);
    }
}
