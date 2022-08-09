// Copyright 2020 Peter Williams <peter@newton.cx> and collaborators
// Licensed under the MIT License.

//! Helpers for environment variables.

use anyhow::anyhow;
use std::env;

use crate::errors::Result;

/// Get an optional environment variable as a string.
///
/// If the variable is not present or is empty, return `Ok(None)`. If the
/// variable is present but cannot be converted into a string, return an `Err`.
pub fn maybe_var(key: &str) -> Result<Option<String>> {
    if let Some(os_str) = env::var_os(key) {
        if let Ok(s) = os_str.into_string() {
            if !s.is_empty() {
                Ok(Some(s))
            } else {
                Ok(None)
            }
        } else {
            Err(anyhow!(
                "could not parse environment variable {} as Unicode",
                key
            ))
        }
    } else {
        Ok(None)
    }
}

/// Require an environment variable as a string.
///
/// If the variable is not present, or is empty, or cannot be converted into a
/// string, return an `Err`.
pub fn require_var(key: &str) -> Result<String> {
    maybe_var(key)?.ok_or_else(|| anyhow!("environment variable {} must be provided", key))
}
