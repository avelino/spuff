//! Connectivity modules for remote instances.
//!
//! This module provides connection functionality for various providers:
//! - SSH: Pure Rust SSH for cloud providers (DigitalOcean, Hetzner, AWS)
//! - Docker: Docker exec API for local containers

pub mod docker;
pub mod ssh;
