use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::time::{Instant, sleep_until};

use crate::{Client, ClientError, DefaultClient, DefaultConfig, MultiClient, Response, Subrequest};

pub struct HedgingConfig {
    pub latency: Duration,
    pub hedging_delay: Duration,
}

pub struct HedgingClient;

impl MultiClient for HedgingClient {
    type Config = HedgingConfig;

    async fn execute<S: Subrequest>(
        &self,
        senders: Vec<S>,
        config: HedgingConfig,
    ) -> Result<Response, ClientError> {
        let mut senders = senders.into_iter();
        let first = match senders.next() {
            Some(sender) => sender,
            None => return Err(ClientError::InvalidConfig("Список серверов пуст")),
        };
        if config.latency.is_zero() {
            return Err(ClientError::LatencyBudgetExhausted);
        }

        let start = Instant::now();
        let deadline = start + config.latency;
        let hedge_at = start + config.hedging_delay.min(config.latency);
        let client = DefaultClient;
        let mut requests = FuturesUnordered::new();
        requests.push(client.execute(
            first,
            DefaultConfig {
                latency: config.latency,
            },
        ));
        let mut hedged = false;

        loop {
            tokio::select! {
                biased;
                _ = sleep_until(deadline) => {
                    return Err(ClientError::LatencyBudgetExhausted);
                }
                result = requests.next(), if !requests.is_empty() => {
                    if Instant::now() >= deadline {
                        return Err(ClientError::LatencyBudgetExhausted);
                    }
                    if let Some(Ok(response)) = result {
                        return Ok(response);
                    }
                    if hedged {
                        continue;
                    }
                }
                _ = sleep_until(hedge_at), if !hedged => {}
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ClientError::LatencyBudgetExhausted);
            }
            hedged = true;
            for sender in senders.by_ref() {
                requests.push(client.execute(sender, DefaultConfig { latency: remaining }));
            }
        }
    }
}
