use std::io;

use crate::{Operation, Phase};

#[derive(Clone, Copy, Debug)]
pub(crate) enum ResourceKind {
    Handle,
    Process,
    Thread,
    Job,
    Pipe,
    RemoteHandle,
}

#[cfg(feature = "tracing")]
#[derive(Clone, Copy, Debug)]
enum Outcome {
    Success,
    Failure,
}

#[cfg(feature = "tracing")]
pub(crate) fn io<T>(
    phase: Phase,
    operation: Operation,
    resource: ResourceKind,
    call: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let start = std::time::Instant::now();
    let result = call();
    let (outcome, win32_code) = match &result {
        Ok(_) => (Outcome::Success, 0),
        Err(error) => {
            let code: u32 = error
                .raw_os_error()
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or_default();
            (Outcome::Failure, code)
        }
    };
    let elapsed = start.elapsed().as_micros();
    let duration_micros = u64::try_from(elapsed).unwrap_or(u64::MAX);
    tracing::event!(
        target: "windows_spawn",
        tracing::Level::TRACE,
        phase = ?phase,
        operation = ?operation,
        resource = ?resource,
        outcome = ?outcome,
        win32_code,
        duration_micros
    );
    result
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn io<T>(
    _phase: Phase,
    _operation: Operation,
    _resource: ResourceKind,
    call: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    call()
}
