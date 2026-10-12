// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs async D-Bus code (`ashpd`, `notify-rust`) behind `system`'s
//! synchronous API.

use std::future::Future;
use std::time::Duration;

use crate::error::{Error, Result};

/// Runs `future` on a private current-thread runtime, keeping `system`'s API
/// synchronous, and gives up after `timeout`.
///
/// Starting a runtime inside another one panics, so when called from inside a
/// tokio runtime (including `spawn_blocking` threads) the future runs on a
/// short-lived thread of its own instead. `error` builds the caller's error
/// variant from a message.
///
/// The runtime lives only for this call, so the future must not reuse D-Bus
/// connections from other runtimes: `ashpd`'s shared connection dies with the
/// runtime that created it (possibly `rfd`'s) and later calls on it wait
/// forever. Callers open their own connection (`with_connection`).
pub(crate) fn block_on<T: Send>(
    future: impl Future<Output = Result<T>> + Send,
    timeout: Duration,
    error: impl Fn(String) -> Error + Sync,
) -> Result<T> {
    let run = || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| error(format!("failed to start the async runtime: {e}")))?;
        runtime
            // The timer must be created inside the runtime.
            .block_on(async { tokio::time::timeout(timeout, future).await })
            .unwrap_or_else(|_| Err(error(format!("no answer from D-Bus after {timeout:?}"))))
    };
    if tokio::runtime::Handle::try_current().is_err() {
        return run();
    }
    std::thread::scope(|scope| {
        scope
            .spawn(run)
            .join()
            .unwrap_or_else(|_| Err(error("the D-Bus call stopped unexpectedly".to_owned())))
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn fail(message: String) -> Error {
        Error::Notification { message }
    }

    const LONG: Duration = Duration::from_secs(10);

    #[test]
    fn runs_outside_a_runtime() {
        assert_eq!(block_on(async { Ok(1) }, LONG, fail).unwrap(), 1);
    }

    #[tokio::test]
    async fn runs_inside_a_runtime() {
        assert_eq!(block_on(async { Ok(2) }, LONG, fail).unwrap(), 2);
    }

    #[test]
    fn gives_up_after_the_timeout() {
        let result: Result<()> = block_on(
            async {
                tokio::time::sleep(Duration::from_mins(1)).await;
                Ok(())
            },
            Duration::from_millis(50),
            fail,
        );
        assert!(
            matches!(result, Err(Error::Notification { message }) if message.contains("no answer"))
        );
    }

    #[test]
    fn runs_inside_spawn_blocking() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let value = runtime
            .block_on(runtime.spawn_blocking(|| block_on(async { Ok(3) }, LONG, fail)))
            .unwrap()
            .unwrap();
        assert_eq!(value, 3);
    }
}
