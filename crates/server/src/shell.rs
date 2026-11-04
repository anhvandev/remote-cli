//! Live interactive shell sessions: routes each session's Agent output to the Controller stream that opened it.

use std::{collections::HashMap, sync::Mutex};

use rcli_protocol::agent::agent_message;
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Default)]
pub struct Shells(Mutex<HashMap<Uuid, Shell>>);

pub struct Shell {
    pub device: Uuid,
    /// Only the user who opened it may type into it.
    pub owner: Uuid,
    output: mpsc::UnboundedSender<agent_message::Msg>,
}

impl Shells {
    /// Registers a session; its ShellOutput/ShellExit arrive on the returned receiver, which ends when the Agent disconnects.
    pub fn open(&self, session: Uuid, device: Uuid, owner: Uuid) -> mpsc::UnboundedReceiver<agent_message::Msg> {
        let (output, rx) = mpsc::unbounded_channel();
        self.0.lock().unwrap().insert(session, Shell { device, owner, output });
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
    pub fn deliver(&self, device: Uuid, session: &str, msg: agent_message::Msg) {
        let Ok(session) = session.parse() else { return };
        if let Some(s) = self.0.lock().unwrap().get(&session).filter(|s| s.device == device) {
            let _ = s.output.send(msg);
        }
    }

    /// The Device's connection ended: its sessions are gone (the Agent kills them too).
    pub fn device_gone(&self, device: Uuid) {
        self.0.lock().unwrap().retain(|_, s| s.device != device);
    }
}
