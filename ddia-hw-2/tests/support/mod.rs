use std::cell::Cell;
use std::collections::VecDeque;
use std::future::pending;
use std::rc::Rc;
use std::time::Duration;

use ddia_hw_2::{Response, Subrequest};

pub enum Outcome {
    Reply(Response),
    Delayed(Duration, Response),
    Hang,
}

#[derive(Default)]
pub struct Probe {
    pub calls: Cell<usize>,
    pub active: Cell<usize>,
}

pub struct ScriptedServer {
    outcomes: VecDeque<Outcome>,
    probe: Rc<Probe>,
}

impl ScriptedServer {
    pub fn new(outcomes: Vec<Outcome>) -> (Self, Rc<Probe>) {
        let probe = Rc::new(Probe::default());
        let server = Self {
            outcomes: VecDeque::from(outcomes),
            probe: probe.clone(),
        };
        (server, probe)
    }
}

struct ActiveRequest(Rc<Probe>);

// Счётчик уменьшается и при ответе, и при отмене ожидания.
impl Drop for ActiveRequest {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
    }
}

impl Subrequest for ScriptedServer {
    async fn send(&mut self) -> Response {
        self.probe.calls.set(self.probe.calls.get() + 1);
        let outcome = self.outcomes.pop_front().expect("Лишний подзапрос");
        self.probe.active.set(self.probe.active.get() + 1);
        let _active = ActiveRequest(self.probe.clone());

        match outcome {
            Outcome::Reply(response) => response,
            Outcome::Delayed(delay, response) => {
                tokio::time::sleep(delay).await;
                response
            }
            Outcome::Hang => pending().await,
        }
    }
}
