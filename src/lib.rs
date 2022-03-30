#![deny(unused)]

pub mod client;
pub mod client_builder;
pub mod connection;
mod connection_config;
pub mod frame;
pub mod header;
pub mod message_builder;
pub mod option_setter;
mod parsing;
mod shutdown;
pub mod subscription;
pub mod subscription_builder;
mod transaction;

/// Error returned by most functions.
///
/// For performance reasons, boxing is avoided in any hot path. For example, in
/// `parse`, a custom error `enum` is defined. This is because the error is hit
/// and handled during normal execution when a partial frame is received on a
/// socket. `std::error::Error` is implemented for `parse::Error` which allows
/// it to be converted to `Box<dyn std::error::Error>`.
// pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub type Result<T> = std::result::Result<T, client::Error>;
