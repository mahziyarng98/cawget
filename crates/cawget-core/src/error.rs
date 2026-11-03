//! Error type shared by the whole crate.

use std::io;

/// Everything that can go wrong in the engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("no such job: {0}")]
    NoSuchJob(i64),
    #[error("job {id} is {state}")]
    WrongState { id: i64, state: String },
    /// Another process (CLI `run` or the GUI) is already processing this queue.
    #[error(
        "another {} instance (`cawget-cli run` or the {} app) is already processing this queue",
        crate::product_name!(),
        crate::product_name!()
    )]
    Busy,
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }
}
