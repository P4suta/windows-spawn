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

pub(crate) fn io<T>(
    phase: Phase,
    operation: Operation,
    resource: ResourceKind,
    call: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    #[cfg(feature = "tracing")]
    let start = std::time::Instant::now();
    let result = call();
    #[cfg(feature = "tracing")]
    {
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
    }
    #[cfg(not(feature = "tracing"))]
    let _ = (phase, operation, resource);
    result
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn io_invokes_the_call_and_preserves_both_result_variants() -> io::Result<()> {
        let mut called = false;
        let value = io(
            Phase::Runtime,
            Operation::ReadPipe,
            ResourceKind::Pipe,
            || {
                called = true;
                Ok(41_u8)
            },
        )?;
        assert!(called);
        assert_eq!(value, 41);
        let error = io(
            Phase::Runtime,
            Operation::WritePipe,
            ResourceKind::Pipe,
            || Err::<(), _>(io::Error::from_raw_os_error(5)),
        )
        .err()
        .ok_or_else(|| io::Error::other("traced failure expected"))?;
        assert_eq!(error.raw_os_error(), Some(5));
        Ok(())
    }
}
