//! # cawget-core
//!
//! A segmented HTTP/HTTPS download engine.
//!
//! * [`segment`]: pure range planning, progress tracking and work stealing
//! * [`store`]: SQLite persistence of settings, jobs and per-segment progress
//! * [`job`]: downloading one file with parallel range requests, resume and checksum
//! * [`manager`]: queue, scheduler, concurrency limit and the API used by the CLI and the GUI
//!
//! ```no_run
//! use cawget_core::{AddRequest, Manager, RunOptions};
//! # async fn demo() -> cawget_core::Result<()> {
//! let m = Manager::open(std::path::Path::new("cawget.db"))?;
//! m.add(AddRequest { url: "https://example.org/big.iso".into(), ..Default::default() })?;
//! let summary = m.run(RunOptions::default(), Default::default()).await?;
//! println!("{} downloaded, {} failed", summary.done, summary.failed);
//! # Ok(()) }
//! ```

/// The product name for engine messages and the default User-Agent. A macro so it can be used in `concat!`.
/// It must equal `productName` in `gui/src-tauri/tauri.conf.json`; `gui/scripts/check-brand.mjs` enforces that.
#[macro_export]
macro_rules! product_name {
    () => {
        "Cawget"
    };
}

/// Default User-Agent: `<product>/<version>`.
pub const DEFAULT_USER_AGENT: &str = concat!(product_name!(), "/", env!("CARGO_PKG_VERSION"));

pub mod categories;
pub mod error;
pub mod http;
pub mod job;
pub mod limiter;
pub mod manager;
pub mod paths;
pub mod secrets;
pub mod segment;
pub mod settings;
pub mod store;
pub mod util;

pub use error::{Error, Result};
pub use manager::{AddRequest, Event, JobView, Manager, ProbeInfo, RunOptions, RunSummary};
pub use paths::Migration;
pub use segment::Segment;
pub use settings::{SETTINGS, Settings};
pub use store::{Job, State, Store};
