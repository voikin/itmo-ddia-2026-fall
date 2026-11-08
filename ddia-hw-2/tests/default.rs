mod support;

use std::time::Duration;

use ddia_hw_2::{Client, ClientError, DefaultClient, DefaultConfig, Response};
use support::{Outcome, ScriptedServer};
use tokio::time::Instant;

fn response(status: u16) -> Response {
    Response {
        status,
        body: b"response body".to_vec(),
    }
}

fn budget(millis: u64) -> DefaultConfig {
    DefaultConfig {
        latency: Duration::from_millis(millis),
    }
}

#[tokio::test(start_paused = true)]
async fn success_returns_response() {
    let expected = response(201);
    let (sender, probe) = ScriptedServer::new(vec![Outcome::Reply(expected.clone())]);
    let result = DefaultClient.execute(sender, budget(100)).await;

    assert_eq!(result, Ok(expected));
    assert_eq!(probe.calls.get(), 1);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn no_retry_on_4xx() {
    let expected = response(429);
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Reply(expected.clone()),
        Outcome::Reply(response(200)),
    ]);
    let result = DefaultClient.execute(sender, budget(100)).await;

    assert_eq!(result, Err(ClientError::Http(expected)));
    assert_eq!(probe.calls.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn no_retry_on_5xx() {
    let expected = response(503);
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Reply(expected.clone()),
        Outcome::Reply(response(200)),
    ]);
    let result = DefaultClient.execute(sender, budget(100)).await;

    assert_eq!(result, Err(ClientError::Http(expected)));
    assert_eq!(probe.calls.get(), 1);
}

#[tokio::test(start_paused = true)]
async fn timeout_cancels_subrequest() {
    for millis in [20, 200] {
        let (sender, probe) = ScriptedServer::new(vec![Outcome::Hang]);
        let start = Instant::now();
        let result = DefaultClient.execute(sender, budget(millis)).await;

        assert_eq!(result, Err(ClientError::LatencyBudgetExhausted));
        // Допуск в 1 мс соответствует точности таймера Tokio.
        assert!(start.elapsed() >= Duration::from_millis(millis));
        assert!(start.elapsed() <= Duration::from_millis(millis + 1));
        assert_eq!(probe.calls.get(), 1);
        assert_eq!(probe.active.get(), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn response_within_budget() {
    let expected = response(200);
    let (sender, probe) = ScriptedServer::new(vec![Outcome::Delayed(
        Duration::from_millis(10),
        expected.clone(),
    )]);
    let result = DefaultClient.execute(sender, budget(100)).await;

    assert_eq!(result, Ok(expected));
    assert_eq!(probe.calls.get(), 1);
    assert_eq!(probe.active.get(), 0);
}
