use std::time::Duration;

use crate::retries::execute_requests;
use crate::{ClientError, MultiClient, Response, RetryConfig, Subrequest};

pub struct CircuitConfig {
    pub failure_threshold: usize,
    pub cooldown: Duration,
}

pub struct CircuitBreakerConfig {
    pub retry: RetryConfig,
    pub circuit: CircuitConfig,
}

pub struct CircuitBreakerClient;

impl MultiClient for CircuitBreakerClient {
    type Config = CircuitBreakerConfig;

    async fn execute<S: Subrequest>(
        &self,
        senders: Vec<S>,
        config: CircuitBreakerConfig,
    ) -> Result<Response, ClientError> {
        if config.circuit.failure_threshold == 0 {
            return Err(ClientError::InvalidConfig(
                "Порог ошибок должен быть больше нуля",
            ));
        }
        if config.circuit.cooldown.is_zero() {
            return Err(ClientError::InvalidConfig(
                "Время отключения должно быть больше нуля",
            ));
        }

        execute_requests(senders, config.retry, Duration::ZERO, Some(config.circuit)).await
    }
}
