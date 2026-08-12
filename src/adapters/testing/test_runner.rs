use crate::domain::tests::{TestEvent, TestStatus};
use crate::ports::test_runner::TestRunnerPort;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

pub struct CargoTestRunnerAdapter;

impl Default for CargoTestRunnerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CargoTestRunnerAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TestRunnerPort for CargoTestRunnerAdapter {
    async fn list_tests(&self) -> Vec<String> {
        // Run cargo test -- --list
        let output = Command::new("cargo")
            .arg("test")
            .arg("--")
            .arg("--list")
            .output()
            .await;

        match output {
            Ok(out) => {
                let s = String::from_utf8_lossy(&out.stdout);
                s.lines()
                    .filter(|l| l.ends_with(": test") || l.ends_with(": benchmark"))
                    .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
                    .collect()
            }
            Err(_) => Vec::new(),
        }
    }

    async fn run_tests(&self) -> mpsc::Receiver<TestEvent> {
        let (tx, rx) = mpsc::channel(100);

        tokio::spawn(async move {
            let mut cmd = Command::new("cargo");
            cmd.arg("test")
                .arg("--")
                .arg("--nocapture") // We want raw output or we use json?
                // Using JSON format is better for parsing, but might be unstable or require -Z
                // Standard `cargo test` output is human readable but harder to parse accurately.
                // However, `cargo test --message-format=json` gives build artifacts, not test execution events in a simple way for everything.
                // The most reliable way for a TUI without unstable features is to stream stdout and parse "test ... ok/FAILED".
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            if let Ok(mut child) = cmd.spawn() {
                let stdout = match child.stdout.take() {
                    Some(s) => s,
                    None => {
                        eprintln!("Failed to open stdout");
                        return;
                    }
                };
                let mut reader = BufReader::new(stdout).lines();

                while let Ok(Some(line)) = reader.next_line().await {
                    // Logic to parse line
                    // "test [name] ... ok"
                    // "test [name] ... FAILED"
                    if line.starts_with("test ")
                        && (line.ends_with("... ok")
                            || line.ends_with("... FAILED")
                            || line.ends_with("... ignored"))
                    {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 4 {
                            let name = parts[1];
                            let status = if line.ends_with("... ok") {
                                TestStatus::Passed
                            } else if line.ends_with("... FAILED") {
                                TestStatus::Failed
                            } else {
                                TestStatus::Ignored
                            };
                            let _ = tx.send(TestEvent::Finished(name.to_string(), status)).await;
                        }
                    } else if line.starts_with("test ") && line.ends_with(" ...") {
                        // Basic running indicator
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 2 {
                            let name = parts[1];
                            let _ = tx.send(TestEvent::Started(name.to_string())).await;
                        }
                    }

                    // Streaming generic log
                    let _ = tx.send(TestEvent::Output("system".to_string(), line)).await;
                }

                let _ = child.wait().await;
            }

            let _ = tx.send(TestEvent::SuiteFinished).await;
        });

        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_constructor() {
        let adapter = CargoTestRunnerAdapter::default();
        let _ = adapter;
    }

    #[test]
    fn test_new_constructor() {
        let adapter = CargoTestRunnerAdapter::new();
        let _ = adapter;
    }
}
