use crate::domain::tests::TestEvent;
use async_trait::async_trait;
use tokio::sync::mpsc;

#[async_trait]
pub trait TestRunnerPort: Send + Sync {
    async fn run_tests(&self) -> mpsc::Receiver<TestEvent>;
    async fn list_tests(&self) -> Vec<String>;
}

#[cfg(test)]
mockall::mock! {
    pub TestRunnerPort {}
    #[async_trait]
    impl TestRunnerPort for TestRunnerPort {
        async fn run_tests(&self) -> mpsc::Receiver<TestEvent>;
        async fn list_tests(&self) -> Vec<String>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_test_runner_list_tests() {
        let mut mock = MockTestRunnerPort::new();
        mock.expect_list_tests()
            .returning(|| vec!["test_one".into(), "test_two".into()]);
        let tests = mock.list_tests().await;
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[0], "test_one");
    }

    #[tokio::test]
    async fn test_mock_test_runner_run_tests() {
        let mut mock = MockTestRunnerPort::new();
        mock.expect_run_tests().returning(|| {
            let (tx, rx) = mpsc::channel(10);
            // Simulate sending a few events
            tokio::spawn(async move {
                let _ = tx.send(TestEvent::Started("test1".into())).await;
                let _ = tx
                    .send(TestEvent::Finished(
                        "test1".into(),
                        crate::domain::tests::TestStatus::Passed,
                    ))
                    .await;
                let _ = tx.send(TestEvent::SuiteFinished).await;
            });
            rx
        });
        let mut rx = mock.run_tests().await;
        let event = rx.recv().await;
        assert!(event.is_some());
    }
}
