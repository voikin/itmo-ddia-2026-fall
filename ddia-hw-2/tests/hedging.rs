mod support;

use std::rc::Rc;
use std::time::Duration;

use ddia_hw_2::{ClientError, HedgingClient, HedgingConfig, MultiClient, Response, Subrequest};
use support::{Outcome, Probe, ScriptedServer};
use tokio::time::Instant;

fn response(status: u16) -> Response {
    Response {
        status,
        body: b"response body".to_vec(),
    }
}

fn config() -> HedgingConfig {
    HedgingConfig {
        latency: Duration::from_millis(100),
        hedging_delay: Duration::from_millis(10),
    }
}

fn assert_elapsed(start: Instant, millis: u64) {
    assert!(start.elapsed() >= Duration::from_millis(millis));
    assert!(start.elapsed() <= Duration::from_millis(millis + 2));
}

fn assert_finished(probes: &[Rc<Probe>], calls: &[usize]) {
    assert_eq!(probes.len(), calls.len());
    for (probe, expected) in probes.iter().zip(calls) {
        assert_eq!(probe.calls.get(), *expected);
        assert_eq!(probe.active.get(), 0);
    }
}

struct TimedServer {
    server: ScriptedServer,
    start: Instant,
    expected_start: u64,
}

impl Subrequest for TimedServer {
    async fn send(&mut self) -> Response {
        assert_elapsed(self.start, self.expected_start);
        self.server.send().await
    }
}

#[tokio::test(start_paused = true)]
async fn fast_first_response_skips_other_servers() {
    let (first, first_probe) = ScriptedServer::new(vec![Outcome::Delayed(
        Duration::from_millis(5),
        response(201),
    )]);
    let (second, second_probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let start = Instant::now();

    assert_eq!(
        HedgingClient.execute(vec![first, second], config()).await,
        Ok(response(201))
    );
    assert_elapsed(start, 5);
    assert_finished(&[first_probe, second_probe], &[1, 0]);
}

#[tokio::test(start_paused = true)]
async fn remaining_servers_start_together_and_fastest_wins() {
    let (first, first_probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let (second, second_probe) = ScriptedServer::new(vec![Outcome::Delayed(
        Duration::from_millis(30),
        response(201),
    )]);
    let (third, third_probe) = ScriptedServer::new(vec![Outcome::Delayed(
        Duration::from_millis(5),
        response(202),
    )]);
    let start = Instant::now();
    let senders = vec![
        TimedServer {
            server: first,
            start,
            expected_start: 0,
        },
        TimedServer {
            server: second,
            start,
            expected_start: 10,
        },
        TimedServer {
            server: third,
            start,
            expected_start: 10,
        },
    ];

    assert_eq!(
        HedgingClient.execute(senders, config()).await,
        Ok(response(202))
    );
    assert_elapsed(start, 15);
    assert_finished(&[first_probe, second_probe, third_probe], &[1, 1, 1]);
}

#[tokio::test(start_paused = true)]
async fn failed_hedge_does_not_cancel_other_requests() {
    let (first, first_probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let (second, second_probe) = ScriptedServer::new(vec![Outcome::Reply(response(503))]);
    let (third, third_probe) = ScriptedServer::new(vec![Outcome::Delayed(
        Duration::from_millis(20),
        response(200),
    )]);
    let start = Instant::now();

    assert_eq!(
        HedgingClient
            .execute(vec![first, second, third], config())
            .await,
        Ok(response(200))
    );
    assert_elapsed(start, 30);
    assert_finished(&[first_probe, second_probe, third_probe], &[1, 1, 1]);
}

#[tokio::test(start_paused = true)]
async fn all_errors_wait_for_total_latency() {
    let (first, first_probe) = ScriptedServer::new(vec![Outcome::Reply(response(400))]);
    let (second, second_probe) = ScriptedServer::new(vec![Outcome::Reply(response(503))]);
    let start = Instant::now();

    assert_eq!(
        HedgingClient.execute(vec![first, second], config()).await,
        Err(ClientError::LatencyBudgetExhausted)
    );
    assert_elapsed(start, 100);
    assert_finished(&[first_probe, second_probe], &[1, 1]);
}

#[tokio::test(start_paused = true)]
async fn timeout_cancels_all_pending_requests() {
    let (first, first_probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let (second, second_probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let (third, third_probe) = ScriptedServer::new(vec![Outcome::Hang]);
    let start = Instant::now();

    assert_eq!(
        HedgingClient
            .execute(vec![first, second, third], config())
            .await,
        Err(ClientError::LatencyBudgetExhausted)
    );
    assert_elapsed(start, 100);
    assert_finished(&[first_probe, second_probe, third_probe], &[1, 1, 1]);
}
