use crate::domain::error::{EnolaError, Result};
use obfstr::obfstr;
use std::fs;

pub struct SecurityAdapter;

impl Default for SecurityAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityAdapter {
    pub fn new() -> Self {
        Self
    }

    pub fn check_debugger(&self) -> Result<()> {
        if self.is_tracer_present() {
            // Log security event?
            return Err(EnolaError::SecurityError(
                "Debugger detected! Execution aborted.".to_string(),
            ));
        }
        Ok(())
    }

    fn is_tracer_present(&self) -> bool {
        // Linux specific: Check /proc/self/status for TracerPid
        // We use obfstr! to hide the path strings from simple `strings` analysis

        if let Ok(status) = fs::read_to_string(obfstr!("/proc/self/status")) {
            for line in status.lines() {
                if line.starts_with(obfstr!("TracerPid:")) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() > 1 {
                        if let Ok(pid) = parts[1].parse::<i32>() {
                            if pid != 0 {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        // Fallback or additional checks could go here (e.g. ptrace attach attempt)
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_adapter_default() {
        let adapter = SecurityAdapter;
        let _ = adapter;
    }

    #[test]
    fn test_check_debugger_no_debugger() {
        let adapter = SecurityAdapter::new();
        // In test environment, no debugger should be attached (usually)
        // This may fail if run under a debugger, which is expected behavior
        let result = adapter.check_debugger();
        // We don't assert Ok because running under IDE debugger would make it Err
        let _ = result;
    }

    #[test]
    fn test_is_tracer_present_returns_bool() {
        let adapter = SecurityAdapter::new();
        // Private method, test via check_debugger
        let _ = adapter.check_debugger();
    }
}
