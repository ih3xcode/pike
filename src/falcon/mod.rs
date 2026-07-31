//! Клієнт CrowdStrike Falcon API. Реалізує порти з [`crate::sensors::ports`],
//! тому решта коду може працювати з ним, нічого не знаючи про HTTP і OAuth2.

pub mod auth;
pub mod client;

pub use client::FalconClient;
