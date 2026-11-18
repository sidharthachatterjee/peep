//! Peep's own threads for network work, so none of it runs on the thread that draws.

use std::{io, sync::OnceLock};

use tokio::runtime::Runtime;

/// The threads all network work runs on, started on first use.
pub(crate) fn runtime() -> io::Result<&'static Runtime> {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("peep-network")
        .enable_all()
        .build()?;
    Ok(RUNTIME.get_or_init(|| runtime))
}
