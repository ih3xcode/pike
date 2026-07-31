//! The CrowdStrike Falcon API client. It implements the ports from
//! [`crate::sensors::ports`], so the rest of the code can work with it
//! knowing nothing about HTTP or OAuth2.

pub mod auth;
pub mod client;

pub use client::FalconClient;
