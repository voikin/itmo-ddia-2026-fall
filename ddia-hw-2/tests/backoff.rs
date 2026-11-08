mod support;

use std::collections::VecDeque;
use std::time::Duration;

use ddia_hw_2::{
    BackoffClient, BackoffConfig, Client, ClientError, Response, RetryConfig, Subrequest,
};
use support::{Outcome, ScriptedServer};
use tokio::time::Instant;

fn response(status: u16) -> Response {
    Response {
        status,
        body: b"response body".to_vec(),
    }
}

fn config() -> BackoffConfig {
    BackoffConfig {
        retry: RetryConfig {
            max_retries: 5,
            fast_errors: 4,
            failures: 4,
            timeouts: 4,
            latency: Duration::from_millis(200),
            subrequest_latency: Duration::from_millis(20),
        },
        initial_delay: Duration::from_millis(10),
    }
}

struct TimedServer {
    server: ScriptedServer,
    start: Instant,
    expected_times: VecDeque<u64>,
}

impl Subrequest for TimedServer {
    async fn send(&mut self) -> Response {
        let expected = self.expected_times.pop_front().expect("Лишний подзапрос");
        let elapsed = self.start.elapsed();
        assert!(elapsed >= Duration::from_millis(expected));
        // Небольшой допуск на точность нескольких последовательных таймеров.
        assert!(elapsed <= Duration::from_millis(expected + 5));
        self.server.send().await
    }
}

#[tokio::test(start_paused = true)]
async fn delays_double_after_5xx() {
    let (server, probe) = ScriptedServer::new(vec![
        Outcome::Reply(response(500)),
        Outcome::Reply(response(502)),
        Outcome::Reply(response(503)),
        Outcome::Reply(response(200)),
    ]);
    let sender = TimedServer {
        server,
        start: Instant::now(),
        expected_times: VecDeque::from(vec![0, 10, 30, 70]),
    };

    assert_eq!(
        BackoffClient.execute(sender, config()).await,
        Ok(response(200))
    );
    assert_eq!(probe.calls.get(), 4);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn timeouts_and_5xx_share_delay_sequence() {
    let (server, probe) = ScriptedServer::new(vec![
        Outcome::Hang,
        Outcome::Reply(response(503)),
        Outcome::Hang,
        Outcome::Reply(response(200)),
    ]);
    let sender = TimedServer {
        server,
        start: Instant::now(),
        expected_times: VecDeque::from(vec![0, 30, 50, 110]),
    };

    assert_eq!(
        BackoffClient.execute(sender, config()).await,
        Ok(response(200))
    );
    assert_eq!(probe.calls.get(), 4);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn fast_errors_do_not_delay_or_change_backoff() {
    let (server, probe) = ScriptedServer::new(vec![
        Outcome::Reply(response(400)),
        Outcome::Reply(response(503)),
        Outcome::Reply(response(429)),
        Outcome::Reply(response(500)),
        Outcome::Reply(response(200)),
    ]);
    let sender = TimedServer {
        server,
        start: Instant::now(),
        expected_times: VecDeque::from(vec![0, 0, 10, 10, 30]),
    };

    assert_eq!(
        BackoffClient.execute(sender, config()).await,
        Ok(response(200))
    );
    assert_eq!(probe.calls.get(), 5);
}

#[tokio::test(start_paused = true)]
async fn delay_reduces_time_for_next_attempt() {
    let (sender, probe) = ScriptedServer::new(vec![
        Outcome::Reply(response(503)),
        Outcome::Delayed(Duration::from_millis(20), response(200)),
    ]);
    let mut settings = config();
    settings.retry.latency = Duration::from_millis(25);
    let start = Instant::now();

    assert_eq!(
        BackoffClient.execute(sender, settings).await,
        Err(ClientError::LatencyBudgetExhausted),
    );
    assert!(start.elapsed() >= Duration::from_millis(25));
    assert!(start.elapsed() <= Duration::from_millis(26));
    assert_eq!(probe.calls.get(), 2);
    assert_eq!(probe.active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn exhausted_budgets_do_not_add_delay() {
    for status in [400, 503] {
        let (sender, probe) = ScriptedServer::new(vec![Outcome::Reply(response(status))]);
        let mut settings = config();
        settings.retry.fast_errors = 1;
        settings.retry.failures = 1;
        let start = Instant::now();

        let expected = if status == 400 {
            ClientError::FastErrorsBudgetExhausted
        } else {
            ClientError::FailuresBudgetExhausted
        };
        assert_eq!(BackoffClient.execute(sender, settings).await, Err(expected));
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert_eq!(probe.calls.get(), 1);
    }

    let (sender, probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let mut settings = config();
    settings.retry.timeouts = 1;
    let start = Instant::now();

    assert_eq!(
        BackoffClient.execute(sender, settings).await,
        Err(ClientError::TimeoutBudgetExhausted),
    );
    assert!(start.elapsed() >= Duration::from_millis(20));
    assert!(start.elapsed() <= Duration::from_millis(21));
    assert_eq!(probe.calls.get(), 1);
    assert_eq!(probe.active.get(), 0);
}
