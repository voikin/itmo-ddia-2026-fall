use std::time::Duration;

use crate::{Client, ClientError, Response, Subrequest};

pub struct DefaultConfig {
    pub latency: Duration,
}

pub struct DefaultClient;

impl Client for DefaultClient {
    type Config = DefaultConfig;

    async fn execute<S: Subrequest>(
        &self,
        mut sender: S,
        config: DefaultConfig,
    ) -> Result<Response, ClientError> {
        if config.latency.is_zero() {
            return Err(ClientError::LatencyBudgetExhausted);
        }

        match tokio::time::timeout(config.latency, sender.send()).await {
            Ok(response) => {
                if (200..300).contains(&response.status) {
                    Ok(response)
                } else {
                    Err(ClientError::Http(response))
                }
            }
            Err(_) => Err(ClientError::LatencyBudgetExhausted),
        }
    }
}
