use std::time::Duration;

use tokio::time::Instant;

use crate::{CircuitConfig, Client, ClientError, Response, Subrequest};

pub struct RetryConfig {
    pub max_retries: usize,
    pub fast_errors: usize,
    pub failures: usize,
    pub timeouts: usize,
    pub latency: Duration,
    pub subrequest_latency: Duration,
}

pub struct RetryClient;

impl Client for RetryClient {
    type Config = RetryConfig;

    async fn execute<S: Subrequest>(
        &self,
        sender: S,
        config: RetryConfig,
    ) -> Result<Response, ClientError> {
        execute_retries(sender, config, Duration::ZERO).await
    }
}

pub(crate) async fn execute_retries<S: Subrequest>(
    sender: S,
    config: RetryConfig,
    initial_delay: Duration,
) -> Result<Response, ClientError> {
    execute_requests(vec![sender], config, initial_delay, None).await
}

pub(crate) async fn execute_requests<S: Subrequest>(
    mut senders: Vec<S>,
    config: RetryConfig,
    initial_delay: Duration,
    circuit: Option<CircuitConfig>,
) -> Result<Response, ClientError> {
    if senders.is_empty() {
        return Err(ClientError::InvalidConfig("Список серверов пуст"));
    }
    if config.subrequest_latency.is_zero() {
        return Err(ClientError::InvalidConfig(
            "Таймаут подзапроса должен быть больше нуля",
        ));
    }
    if config.latency.is_zero() {
        return Err(ClientError::LatencyBudgetExhausted);
    }

    let start = Instant::now();
    let mut fast_errors = 0;
    let mut failures = 0;
    let mut timeouts = 0;
    let mut delay = initial_delay;
    let mut attempts = 0;
    let mut next = 0;
    let mut server_failures = vec![0; senders.len()];
    let mut blocked_at: Vec<Option<Instant>> = vec![None; senders.len()];

    while attempts < config.max_retries {
        let remaining = config.latency.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(ClientError::LatencyBudgetExhausted);
        }

        let mut selected = None;
        let mut wait = remaining;
        for _ in 0..senders.len() {
            let current = next;
            next = (next + 1) % senders.len();

            if let (Some(settings), Some(blocked)) = (&circuit, blocked_at[current]) {
                let cooldown_left = settings.cooldown.saturating_sub(blocked.elapsed());
                if !cooldown_left.is_zero() {
                    wait = wait.min(cooldown_left);
                    continue;
                }
                blocked_at[current] = None;
                server_failures[current] = 0;
            }
            selected = Some(current);
            break;
        }

        let current = match selected {
            Some(index) => index,
            None => {
                let remaining = config.latency.saturating_sub(start.elapsed());
                if remaining.is_zero() {
                    return Err(ClientError::LatencyBudgetExhausted);
                }
                tokio::time::sleep(wait.min(remaining)).await;
                continue;
            }
        };

        let remaining = config.latency.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return Err(ClientError::LatencyBudgetExhausted);
        }
        attempts += 1;
        let timeout = config.subrequest_latency.min(remaining);
        let result = tokio::time::timeout(timeout, senders[current].send()).await;

        if start.elapsed() >= config.latency {
            return Err(ClientError::LatencyBudgetExhausted);
        }

        let failed = match &result {
            Ok(response) => (400..600).contains(&response.status),
            Err(_) => true,
        };
        if failed {
            if let Some(settings) = &circuit {
                server_failures[current] += 1;
                if server_failures[current] >= settings.failure_threshold {
                    blocked_at[current] = Some(Instant::now());
                }
            }
        }

        let mut needs_delay = false;
        match result {
            Ok(response) => match response.status {
                200..=299 => return Ok(response),
                400..=499 => {
                    fast_errors += 1;
                    if fast_errors >= config.fast_errors {
                        return Err(ClientError::FastErrorsBudgetExhausted);
                    }
                }
                500..=599 => {
                    failures += 1;
                    if failures >= config.failures {
                        return Err(ClientError::FailuresBudgetExhausted);
                    }
                    needs_delay = true;
                }
                _ => return Err(ClientError::Http(response)),
            },
            Err(_) => {
                timeouts += 1;
                if timeouts >= config.timeouts {
                    return Err(ClientError::TimeoutBudgetExhausted);
                }
                needs_delay = true;
            }
        }

        if attempts == config.max_retries {
            return Err(ClientError::RetryBudgetExhausted);
        }

        if needs_delay && !delay.is_zero() {
            let remaining = config.latency.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return Err(ClientError::LatencyBudgetExhausted);
            }

            tokio::time::sleep(delay.min(remaining)).await;
            delay = delay.saturating_mul(2);
        }
    }

    Err(ClientError::RetryBudgetExhausted)
}
