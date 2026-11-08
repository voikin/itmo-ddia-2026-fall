mod support;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use ddia_hw_2::{
    CircuitBreakerClient, CircuitBreakerConfig, CircuitConfig, ClientError, MultiClient, Response,
    RetryConfig, Subrequest,
};
use support::{Outcome, Probe, ScriptedServer};
use tokio::time::Instant;

fn response(status: u16) -> Response {
    Response {
        status,
        body: b"response body".to_vec(),
    }
}

fn config() -> CircuitBreakerConfig {
    CircuitBreakerConfig {
        retry: RetryConfig {
            max_retries: 10,
            fast_errors: 10,
            failures: 10,
            timeouts: 10,
            latency: Duration::from_millis(200),
            subrequest_latency: Duration::from_millis(50),
        },
        circuit: CircuitConfig {
            failure_threshold: 2,
            cooldown: Duration::from_millis(30),
        },
    }
}

fn assert_elapsed(start: Instant, millis: u64) {
    assert!(start.elapsed() >= Duration::from_millis(millis));
    assert!(start.elapsed() <= Duration::from_millis(millis + 3));
}

fn assert_finished(probes: &[Rc<Probe>], calls: &[usize]) {
    assert_eq!(probes.len(), calls.len());
    for (probe, expected) in probes.iter().zip(calls) {
        assert_eq!(probe.calls.get(), *expected);
        assert_eq!(probe.active.get(), 0);
    }
}

struct CheckedServer {
    server: ScriptedServer,
    index: usize,
    order: Rc<Vec<usize>>,
    next: Rc<Cell<usize>>,
}

impl Subrequest for CheckedServer {
    async fn send(&mut self) -> Response {
        assert_eq!(self.index, self.order[self.next.get()]);
        self.next.set(self.next.get() + 1);
        self.server.send().await
    }
}

fn servers(
    scenarios: Vec<Vec<Outcome>>,
    order: Vec<usize>,
) -> (Vec<CheckedServer>, Vec<Rc<Probe>>) {
    let order = Rc::new(order);
    let next = Rc::new(Cell::new(0));
    let mut senders = Vec::new();
    let mut probes = Vec::new();
    for (index, scenario) in scenarios.into_iter().enumerate() {
        let (server, probe) = ScriptedServer::new(scenario);
        senders.push(CheckedServer {
            server,
            index,
            order: order.clone(),
            next: next.clone(),
        });
        probes.push(probe);
    }
    (senders, probes)
}

#[tokio::test(start_paused = true)]
async fn recovered_server_is_used_while_another_is_blocked() {
    let (senders, probes) = servers(
        vec![
            vec![
                Outcome::Reply(response(500)),
                Outcome::Reply(response(503)),
                Outcome::Reply(response(500)),
                Outcome::Reply(response(200)),
            ],
            vec![
                Outcome::Delayed(Duration::from_millis(10), response(400)),
                Outcome::Delayed(Duration::from_millis(35), response(429)),
            ],
        ],
        vec![0, 1, 0, 1, 0, 0],
    );
    let start = Instant::now();

    assert_eq!(
        CircuitBreakerClient.execute(senders, config()).await,
        Ok(response(200))
    );
    assert_elapsed(start, 45);
    assert_finished(&probes, &[4, 2]);
}

#[tokio::test(start_paused = true)]
async fn waits_for_recovery_without_spending_attempts() {
    let (senders, probes) = servers(
        vec![
            vec![Outcome::Reply(response(503)), Outcome::Reply(response(200))],
            vec![Outcome::Delayed(Duration::from_millis(10), response(400))],
        ],
        vec![0, 1, 0],
    );
    let mut settings = config();
    settings.circuit.failure_threshold = 1;
    settings.retry.max_retries = 3;
    let start = Instant::now();

    assert_eq!(
        CircuitBreakerClient.execute(senders, settings).await,
        Ok(response(200))
    );
    assert_elapsed(start, 30);
    assert_finished(&probes, &[2, 1]);
}

#[tokio::test(start_paused = true)]
async fn every_error_type_can_open_circuit() {
    for status in [400, 503, 0] {
        let error = if status == 0 {
            Outcome::Hang
        } else {
            Outcome::Reply(response(status))
        };
        let (senders, probes) =
            servers(vec![vec![error, Outcome::Reply(response(200))]], vec![0, 0]);
        let mut settings = config();
        settings.circuit.failure_threshold = 1;
        settings.retry.subrequest_latency = Duration::from_millis(10);
        let start = Instant::now();

        assert_eq!(
            CircuitBreakerClient.execute(senders, settings).await,
            Ok(response(200))
        );
        assert_elapsed(start, if status == 0 { 40 } else { 30 });
        assert_finished(&probes, &[2]);
    }
}

#[tokio::test(start_paused = true)]
async fn mixed_errors_reach_threshold_and_recovery_resets_counter() {
    let (senders, probes) = servers(
        vec![vec![
            Outcome::Reply(response(400)),
            Outcome::Hang,
            Outcome::Reply(response(503)),
            Outcome::Reply(response(200)),
        ]],
        vec![0, 0, 0, 0],
    );
    let mut settings = config();
    settings.retry.subrequest_latency = Duration::from_millis(10);
    let start = Instant::now();

    assert_eq!(
        CircuitBreakerClient.execute(senders, settings).await,
        Ok(response(200))
    );
    assert_elapsed(start, 40);
    assert_finished(&probes, &[4]);
}

#[tokio::test(start_paused = true)]
async fn cooldown_is_limited_by_total_latency() {
    for cooldown in [Duration::from_millis(30), Duration::MAX] {
        let (senders, probes) = servers(
            vec![
                vec![Outcome::Reply(response(503))],
                vec![Outcome::Reply(response(400))],
            ],
            vec![0, 1],
        );
        let mut settings = config();
        settings.circuit.failure_threshold = 1;
        settings.circuit.cooldown = cooldown;
        settings.retry.latency = Duration::from_millis(15);
        let start = Instant::now();

        assert_eq!(
            CircuitBreakerClient.execute(senders, settings).await,
            Err(ClientError::LatencyBudgetExhausted)
        );
        assert_elapsed(start, 15);
        assert_finished(&probes, &[1, 1]);
    }
}
