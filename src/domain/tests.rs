use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TestStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Ignored,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestInfo {
    pub name: String,
    pub status: TestStatus,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TestEvent {
    Started(String),
    Output(String, String), // (test_name, line)
    Finished(String, TestStatus),
    SuiteFinished,
}
