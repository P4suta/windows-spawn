use std::fmt;
use std::marker::PhantomData;
use std::os::windows::io::{AsHandle, BorrowedHandle, OwnedHandle as SystemOwnedHandle};

pub(crate) enum ProcessKind {}
pub(crate) enum ThreadKind {}
pub(crate) enum JobKind {}
pub(crate) enum PipeKind {}
pub(crate) enum IoKind {}
pub(crate) enum RemoteKind {}

#[derive(Clone, Copy, Debug)]
pub(crate) enum CurrentTable {}
#[derive(Clone, Copy, Debug)]
pub(crate) struct AlternateTable<'parent>(PhantomData<&'parent ()>);
#[derive(Clone, Copy, Debug)]
pub(crate) struct SelectedTable<'parent>(PhantomData<(CurrentTable, AlternateTable<'parent>)>);

#[repr(transparent)]
pub(crate) struct ChildHandleValue<Table> {
    raw: isize,
    table: PhantomData<Table>,
}

impl<Table> ChildHandleValue<Table> {
    pub(crate) const fn from_raw(raw: isize) -> Self {
        Self {
            raw,
            table: PhantomData,
        }
    }

    pub(crate) const fn as_raw(self) -> isize {
        self.raw
    }
}

impl<Table> Clone for ChildHandleValue<Table> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Table> Copy for ChildHandleValue<Table> {}

impl<Table> PartialEq for ChildHandleValue<Table> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl<Table> Eq for ChildHandleValue<Table> {}

impl<Table> fmt::Display for ChildHandleValue<Table> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.raw.fmt(formatter)
    }
}

impl<Table> fmt::Debug for ChildHandleValue<Table> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ChildHandleValue")
    }
}

pub(crate) struct OwnedHandle<Kind, Table> {
    inner: SystemOwnedHandle,
    kind: PhantomData<Kind>,
    table: PhantomData<Table>,
}

impl<Kind, Table> OwnedHandle<Kind, Table> {
    pub(crate) fn from_system(inner: SystemOwnedHandle) -> Self {
        Self {
            inner,
            kind: PhantomData,
            table: PhantomData,
        }
    }
}

impl<Kind, Table> AsHandle for OwnedHandle<Kind, Table> {
    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.inner.as_handle()
    }
}

impl<Kind, Table> fmt::Debug for OwnedHandle<Kind, Table> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("OwnedHandle")
            .field(&self.inner)
            .finish()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn child_values_format_without_exposing_debug_numbers() {
        let value = ChildHandleValue::<CurrentTable>::from_raw(42);
        assert_eq!(value.to_string(), "42");
        assert_eq!(format!("{value:?}"), "ChildHandleValue");
        assert_eq!(value.clone(), value);
    }
}
