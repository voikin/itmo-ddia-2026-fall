#![allow(async_fn_in_trait)]

#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum ClientError {
    Http(Response),
    LatencyBudgetExhausted,
    RetryBudgetExhausted,
    FastErrorsBudgetExhausted,
    FailuresBudgetExhausted,
    TimeoutBudgetExhausted,
    InvalidConfig(&'static str),
}

pub trait Subrequest {
    async fn send(&mut self) -> Response;
}

pub trait Client {
    type Config;

    async fn execute<S: Subrequest>(
        &self,
        sender: S,
        config: Self::Config,
    ) -> Result<Response, ClientError>;
}

pub trait MultiClient {
    type Config;

    async fn execute<S: Subrequest>(
        &self,
        senders: Vec<S>,
        config: Self::Config,
    ) -> Result<Response, ClientError>;
}
