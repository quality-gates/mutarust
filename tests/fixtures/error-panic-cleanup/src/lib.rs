use std::cell::Cell;

#[derive(Debug)]
pub struct Cause;

impl ::core::fmt::Display for Cause {
    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        formatter.write_str("cause")
    }
}

impl ::std::error::Error for Cause {}

#[derive(Debug)]
pub struct ObservedWrapper {
    cause: Cause,
}

impl ObservedWrapper {
    pub fn new() -> Self {
        Self { cause: Cause }
    }
}

impl Default for ObservedWrapper {
    fn default() -> Self {
        Self::new()
    }
}

impl ::core::fmt::Display for ObservedWrapper {
    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        formatter.write_str("observed wrapper")
    }
}

impl ::std::error::Error for ObservedWrapper {
    fn source(&self) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> {
        ::core::option::Option::Some(&self.cause)
    }
}

#[derive(Debug)]
pub struct UnobservedWrapper {
    cause: Cause,
}

impl ::core::fmt::Display for UnobservedWrapper {
    fn fmt(&self, formatter: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        formatter.write_str("unobserved wrapper")
    }
}

impl ::std::error::Error for UnobservedWrapper {
    fn source(&self) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> {
        ::std::option::Option::Some(&self.cause)
    }
}

pub fn observed_recovery() -> bool {
    let message = String::from("observed panic");
    ::std::panic::catch_unwind(move || panic!("{message}")).is_err()
}

pub fn unobserved_recovery() -> bool {
    ::std::panic::catch_unwind(|| 7).is_ok()
}

struct Cleanup<'value>(&'value Cell<u8>);

impl ::core::ops::Drop for Cleanup<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

pub fn observed_cleanup(cell: &Cell<u8>) -> u8 {
    let cleanup = Cleanup(cell);
    drop(cleanup);
    cell.get()
}

pub fn unobserved_cleanup(cell: &Cell<u8>) -> u8 {
    let cleanup = Cleanup(cell);
    ::core::mem::drop(cleanup);
    cell.get()
}

struct BorrowCleanup<'value>(&'value mut u8);

impl ::core::ops::Drop for BorrowCleanup<'_> {
    fn drop(&mut self) {
        *self.0 += 1;
    }
}

pub fn lifetime_checked_cleanup() -> u8 {
    let mut value = 0;
    let cleanup = BorrowCleanup(&mut value);
    ::std::mem::drop(cleanup);
    value += 1;
    value
}
