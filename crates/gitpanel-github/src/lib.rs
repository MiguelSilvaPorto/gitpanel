#![forbid(unsafe_code)]

mod auth;
mod client;
mod dto;
mod error;

pub use auth::Token;
pub use client::{CommitPage, FileUpdate, GithubClient};
pub use error::GithubError;
