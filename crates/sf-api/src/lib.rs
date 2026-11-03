//! sf-api: Salesforce transport, wire models and pagination
//!
//! This library handles low-level Salesforce API communication, including
//! HTTP transport, JSON/XML serialization, pagination, and API response models.

mod client;
mod error;
pub mod metadata;
mod origin;
pub mod rest;
pub mod retry;

pub use client::{
    ApiLimitInfo, ApiLimitUsage, ApiResponse, BearerToken, HttpClient, RetryAfter,
    TransportSettings,
};
pub use error::ApiError;
pub use origin::InstanceOrigin;
pub use reqwest::{Method, StatusCode, Url};
