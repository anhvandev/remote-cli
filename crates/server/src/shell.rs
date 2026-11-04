//! Live interactive shell sessions: routes each session's Agent output to the Controller stream that opened it.

use std::{collections::HashMap, sync::Mutex};

use rcli_protocol::agent::agent_message;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Agent messages buffered per session for a Controller that reads slower than the shell prints.
const BUFFER: usize = 256;

#[derive(Default)]
pub struct Shells(Mutex<HashMap<Uuid, Shell>>);

pub struct Shell {
    pub device: Uuid,
    /// Only the user who opened it may type into it.
    pub owner: Uuid,
    /// None once the Controller fell `BUFFER` messages behind: the session is being dropped.
    output: Option<mpsc::Sender<agent_message::Msg>>,
}

impl Shells {
    /// Registers a session; its ShellOutput/ShellExit arrive on the returned receiver, which ends when the Agent disconnects.
    /// The receiver also ends when the Controller falls too far behind (see `overflowed`).
    pub fn open(&self, session: Uuid, device: Uuid, owner: Uuid) -> mpsc::Receiver<agent_message::Msg> {
        let (output, rx) = mpsc::channel(BUFFER);
        self.0.lock().unwrap().insert(session, Shell { device, owner, output: Some(output) });
        rx
    }

    pub fn close(&self, session: Uuid) {
        self.0.lock().unwrap().remove(&session);
    }

    /// The Device of `session` if `user` opened it.
    pub fn device_of(&self, session: Uuid, user: Uuid) -> Option<Uuid> {
        self.0.lock().unwrap().get(&session).filter(|s| s.owner == user).map(|s| s.device)
    }

    /// Output or exit from `device`'s Agent; a Device can only feed its own sessions.
    /// Never waits (it runs in the Agent's session loop): a full buffer drops the session instead.
    pub fn deliver(&self, device: Uuid, session: &str, msg: agent_message::Msg) {
        let Ok(session) = session.parse() else { return };
        if let Some(s) = self.0.lock().unwrap().get_mut(&session).filter(|s| s.device == device) {
            if let Some(o) = &s.output {
                if let Err(mpsc::error::TrySendError::Full(_)) = o.try_send(msg) {
                    s.output = None;
                }
            }
        }
    }

    /// The session was dropped because its Controller did not keep up with the output.
    pub fn overflowed(&self, session: Uuid) -> bool {
        self.0.lock().unwrap().get(&session).is_some_and(|s| s.output.is_none())
    }

    /// The Device's connection ended: its sessions are gone (the Agent kills them too).
    pub fn device_gone(&self, device: Uuid) {
        self.0.lock().unwrap().retain(|_, s| s.device != device);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcli_protocol::agent::ShellOutput;

    #[test]
    fn a_controller_that_stops_reading_loses_its_session_instead_of_buffering_forever() {
        let shells = Shells::default();
        let (session, device) = (Uuid::new_v4(), Uuid::new_v4());
        let mut rx = shells.open(session, device, Uuid::new_v4());
        let out = || agent_message::Msg::ShellOutput(ShellOutput { session: session.to_string(), data: b"x".to_vec() });
        for _ in 0..10_000 {
            shells.deliver(device, &session.to_string(), out());
        }
        let mut got = 0;
        while rx.try_recv().is_ok() {
            got += 1;
        }
        assert!(got < 10_000, "buffered everything");
        assert!(rx.is_closed(), "the session is dropped");
        assert!(shells.overflowed(session));
    }
}
