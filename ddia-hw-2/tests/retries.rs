mod support;

use std::time::Duration;

use ddia_hw_2::{Client, ClientError, Response, RetryClient, RetryConfig};
use support::{Outcome, ScriptedServer};
use tokio::time::Instant;

fn response(status: u16) -> Response {
    Response {
        status,
        body: b"response body".to_vec(),
    }
}

fn config() -> RetryConfig {
    RetryConfig {
        max_retries: 5,
        fast_errors: 3,
        failures: 3,
        timeouts: 3,
        latency: Duration::from_millis(100),
        subrequest_latency: Duration::from_millis(20),
    }
}

#[tokio::test(start_paused = true)]
async fn retries_different_errors_until_success() {
    let expected = response(200);
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Reply(response(429)),
        Outcome::Reply(response(503)),
        Outcome::Hang,
        Outcome::Reply(expected.clone()),
    ]);
    let mut settings = config();
    settings.max_retries = 4;
    settings.fast_errors = 2;
    settings.failures = 2;
    settings.timeouts = 2;

    let result = RetryClient.execute(sender, settings).await;

    assert_eq!(result, Ok(expected));
    assert_eq!(probe.calls.get(), 4);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn fast_errors_stop_at_budget() {
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Reply(response(400)),
        Outcome::Reply(response(429)),
        Outcome::Reply(response(200)),
    ]);
    let mut settings = config();
    settings.fast_errors = 2;

    let result = RetryClient.execute(sender, settings).await;

    assert_eq!(result, Err(ClientError::FastErrorsBudgetExhausted));
    assert_eq!(probe.calls.get(), 2);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn failures_stop_at_budget() {
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Reply(response(500)),
        Outcome::Reply(response(503)),
        Outcome::Reply(response(200)),
    ]);
    let mut settings = config();
    settings.failures = 2;

    let result = RetryClient.execute(sender, settings).await;

    assert_eq!(result, Err(ClientError::FailuresBudgetExhausted));
    assert_eq!(probe.calls.get(), 2);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn timeouts_stop_at_budget() {
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Hang,
        Outcome::Hang,
        Outcome::Reply(response(200)),
    ]);
    let mut settings = config();
    settings.timeouts = 2;

    let result = RetryClient.execute(sender, settings).await;

    assert_eq!(result, Err(ClientError::TimeoutBudgetExhausted));
    assert_eq!(probe.calls.get(), 2);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn total_latency_limits_next_attempt() {
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Delayed(Duration::from_millis(15), response(503)),
        Outcome::Delayed(Duration::from_millis(15), response(200)),
    ]);
    let mut settings = config();
    settings.latency = Duration::from_millis(25);
    let start = Instant::now();

    let result = RetryClient.execute(sender, settings).await;

    assert_eq!(result, Err(ClientError::LatencyBudgetExhausted));
    assert!(start.elapsed() >= Duration::from_millis(25));
    assert!(start.elapsed() <= Duration::from_millis(26));
    assert_eq!(probe.calls.get(), 2);
    assert_eq!(probe.active.get(), 0);
}
