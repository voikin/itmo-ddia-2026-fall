use std::time::Duration;

use crate::retries::execute_retries;
use crate::{Client, ClientError, Response, RetryConfig, Subrequest};

pub struct BackoffConfig {
    pub retry: RetryConfig,
    pub initial_delay: Duration,
}

pub struct BackoffClient;

impl Client for BackoffClient {
    type Config = BackoffConfig;

    async fn execute<S: Subrequest>(
        &self,
        sender: S,
        config: BackoffConfig,
    ) -> Result<Response, ClientError> {
        execute_retries(sender, config.retry, config.initial_delay).await
    }
}
