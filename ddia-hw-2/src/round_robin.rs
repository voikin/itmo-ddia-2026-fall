use std::time::Duration;

use crate::retries::execute_requests;
use crate::{ClientError, MultiClient, Response, RetryConfig, Subrequest};

pub struct RoundRobinClient;

impl MultiClient for RoundRobinClient {
    type Config = RetryConfig;

    async fn execute<S: Subrequest>(
        &self,
        senders: Vec<S>,
        config: RetryConfig,
    ) -> Result<Response, ClientError> {
        execute_requests(senders, config, Duration::ZERO, None).await
    }
}
