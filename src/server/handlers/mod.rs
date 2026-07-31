//! Handlers for the five routes. Each file is one step of the deployment
//! flow: script → callback with facts about the host → binary → report.

pub mod callback;
pub mod done;
pub mod download;
pub mod parse;
pub mod scripts;
