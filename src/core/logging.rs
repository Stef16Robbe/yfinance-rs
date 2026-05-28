#![cfg_attr(not(feature = "tracing"), allow(unused_imports, unused_macros))]

#[cfg(feature = "tracing")]
macro_rules! trace_debug {
    ($($tt:tt)*) => {
        tracing::debug!($($tt)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_debug {
    ($($tt:tt)*) => {};
}

#[cfg(feature = "tracing")]
macro_rules! trace_info {
    ($($tt:tt)*) => {
        tracing::info!($($tt)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_info {
    ($($tt:tt)*) => {};
}

#[cfg(feature = "tracing")]
macro_rules! trace_warn {
    ($($tt:tt)*) => {
        tracing::warn!($($tt)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_warn {
    ($($tt:tt)*) => {};
}

#[cfg(feature = "tracing")]
macro_rules! trace_error {
    ($($tt:tt)*) => {
        tracing::error!($($tt)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_error {
    ($($tt:tt)*) => {};
}

#[cfg(feature = "tracing")]
macro_rules! trace_only {
    ($($tt:tt)*) => {
        $($tt)*
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! trace_only {
    ($($tt:tt)*) => {};
}

pub(crate) use {trace_debug, trace_error, trace_info, trace_only, trace_warn};
