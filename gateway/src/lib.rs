//! rende-gateway: an x402 paywall that sells machine services for USDC on
//! Solana, receive-only.
//!
//! Custody invariant for the whole crate: nothing here holds, derives, or
//! touches a private key. The on-chain surface is two read-only RPC calls.

pub mod adapter;
pub mod config;
pub mod server;
pub mod solana;
