mod support;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use ddia_hw_2::{ClientError, MultiClient, Response, RetryConfig, RoundRobinClient, Subrequest};
use support::{Outcome, Probe, ScriptedServer};
use tokio::time::Instant;

fn response(status: u16) -> Response {
    Response {
        status,
        body: b"response body".to_vec(),
    }
}

fn config() -> RetryConfig {
    RetryConfig {
        max_retries: 10,
        fast_errors: 10,
        failures: 10,
        timeouts: 10,
        latency: Duration::from_millis(200),
        subrequest_latency: Duration::from_millis(20),
    }
}

struct OrderedServer {
    server: ScriptedServer,
    index: usize,
    count: usize,
    next: Rc<Cell<usize>>,
}

impl Subrequest for OrderedServer {
    async fn send(&mut self) -> Response {
        assert_eq!(self.index, self.next.get());
        self.next.set((self.index + 1) % self.count);
        self.server.send().await
    }
}

fn servers(scenarios: Vec<Vec<Outcome>>) -> (Vec<OrderedServer>, Vec<Rc<Probe>>) {
    let count = scenarios.len();
    let next = Rc::new(Cell::new(0));
    let mut senders = Vec::new();
    let mut probes = Vec::new();

    for (index, scenario) in scenarios.into_iter().enumerate() {
        let (server, probe) = ScriptedServer::new(scenario);
        senders.push(OrderedServer {
            server,
            index,
            count,
            next: next.clone(),
        });
        probes.push(probe);
    }

    (senders, probes)
}

#[tokio::test(start_paused = true)]
async fn first_success_stops_rotation() {
    let (senders, probes) = servers(vec![
        vec![Outcome::Reply(response(201))],
        vec![Outcome::Reply(response(200))],
    ]);

    assert_eq!(
        RoundRobinClient.execute(senders, config()).await,
        Ok(response(201))
    );
    assert_eq!(probes[0].calls.get(), 1);
    assert_eq!(probes[1].calls.get(), 0);
    assert_eq!(probes[0].active.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn rotates_in_order_and_wraps_after_errors() {
    let (senders, probes) = servers(vec![
        vec![
            Outcome::Reply(response(500)),
            Outcome::Reply(response(503)),
            Outcome::Reply(response(200)),
        ],
        vec![Outcome::Reply(response(400)), Outcome::Reply(response(500))],
        vec![Outcome::Hang, Outcome::Reply(response(503))],
    ]);

    assert_eq!(
        RoundRobinClient.execute(senders, config()).await,
        Ok(response(200))
    );
    assert_eq!(probes[0].calls.get(), 3);
    assert_eq!(probes[1].calls.get(), 2);
    assert_eq!(probes[2].calls.get(), 2);
    for probe in probes {
        assert_eq!(probe.active.get(), 0);
    }
}

#[tokio::test(start_paused = true)]
async fn error_budgets_are_shared_between_servers() {
    for status in [400, 503, 0] {
        let first;
        let second;
        let expected;
        if status == 0 {
            first = Outcome::Hang;
            second = Outcome::Hang;
            expected = ClientError::TimeoutBudgetExhausted;
        } else {
            first = Outcome::Reply(response(status));
            second = Outcome::Reply(response(status));
            expected = if status == 400 {
                ClientError::FastErrorsBudgetExhausted
            } else {
                ClientError::FailuresBudgetExhausted
            };
        }
        let (senders, probes) = servers(vec![
            vec![first],
            vec![second],
            vec![Outcome::Reply(response(200))],
        ]);
        let mut settings = config();
        settings.fast_errors = 2;
        settings.failures = 2;
        settings.timeouts = 2;

        assert_eq!(
            RoundRobinClient.execute(senders, settings).await,
            Err(expected)
        );
        assert_eq!(probes[0].calls.get(), 1);
        assert_eq!(probes[1].calls.get(), 1);
        assert_eq!(probes[2].calls.get(), 0);
        for probe in probes {
            assert_eq!(probe.active.get(), 0);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn attempts_are_limited_across_servers() {
    let (senders, probes) = servers(vec![
        vec![Outcome::Reply(response(400))],
        vec![Outcome::Reply(response(503))],
        vec![Outcome::Reply(response(200))],
    ]);
    let mut settings = config();
    settings.max_retries = 2;

    assert_eq!(
        RoundRobinClient.execute(senders, settings).await,
        Err(ClientError::RetryBudgetExhausted),
    );
    assert_eq!(probes[0].calls.get(), 1);
    assert_eq!(probes[1].calls.get(), 1);
    assert_eq!(probes[2].calls.get(), 0);
}

#[tokio::test(start_paused = true)]
async fn total_latency_is_shared_between_servers() {
    let (senders, probes) = servers(vec![
        vec![Outcome::Delayed(Duration::from_millis(15), response(503))],
        vec![Outcome::Hang],
        vec![Outcome::Reply(response(200))],
    ]);
    let mut settings = config();
    settings.latency = Duration::from_millis(25);
    let start = Instant::now();

    assert_eq!(
        RoundRobinClient.execute(senders, settings).await,
        Err(ClientError::LatencyBudgetExhausted),
    );
    assert!(start.elapsed() >= Duration::from_millis(25));
    assert!(start.elapsed() <= Duration::from_millis(26));
    assert_eq!(probes[0].calls.get(), 1);
    assert_eq!(probes[1].calls.get(), 1);
    assert_eq!(probes[2].calls.get(), 0);
    for probe in probes {
        assert_eq!(probe.active.get(), 0);
    }
}
