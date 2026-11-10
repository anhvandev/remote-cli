//! The Wayland session adapter: screen capture through the desktop portal's ScreenCast (`screencast`) and input
//! through a virtual device on `/dev/uinput` (`uinput`). Accessibility stays the shared AT-SPI walk in `atspi`.
//!
//! The installed root service cannot call the portal (it lives on the user's session bus) and cannot write to
//! `/dev/uinput` as the session user, so the two halves split: the console-session helper runs this for capture,
//! and the root service sends input through the same `uinput` module (see `session::ConsoleSession`).

use std::{path::PathBuf, sync::Mutex};

use crate::platform::{DisplayInfo, Frame, Input};

use super::{
    screencast::{self, Screen, TokenStore, TOKEN_ENV},
    uinput::VirtualInput,
};

/// Capture and input for the Wayland session this process runs in.
pub struct Wayland {
    /// Where the portal's restore token is kept (the Device state dir), when this process owns it.
    token_file: Option<PathBuf>,
    /// The token the root service handed over for the helper (it cannot read the state dir itself).
    env_token: Option<String>,
    /// Opened on first use. A failed attempt is retried, because the person at the Device may approve later.
    screen: Mutex<Option<Screen>>,
    input: Mutex<Option<VirtualInput>>,
}

impl Wayland {
    pub fn new(token_file: Option<PathBuf>) -> Self {
        let env_token = std::env::var(TOKEN_ENV).ok().map(|token| token.trim().to_owned()).filter(|token| !token.is_empty());
        Self { token_file, env_token, screen: Mutex::new(None), input: Mutex::new(None) }
    }

    pub fn displays(&self) -> anyhow::Result<Vec<DisplayInfo>> {
        self.ensure_screen()?;
        self.screen.lock().unwrap().as_ref().expect("just opened").displays()
    }

    pub fn capture(&self, display: u32) -> anyhow::Result<Frame> {
        self.ensure_screen()?;
        self.screen.lock().unwrap().as_ref().expect("just opened").capture(display)
    }

    pub fn input(&self, input: Input) -> anyhow::Result<()> {
        // Only a move needs the desktop geometry; keys and the wheel go wherever the pointer already is.
        let displays = match input {
            Input::Move { .. } => self.displays()?,
            _ => vec![],
        };
        let mut device = self.input.lock().unwrap();
        if device.is_none() {
            *device = Some(VirtualInput::create()?);
        }
        let out = device.as_mut().expect("just created").input(input, &displays);
        if out.is_err() {
            // A device the compositor dropped (a session restart) is recreated on the next call.
            *device = None;
        }
        out
    }

    /// Opens the portal on first use; the stored token (or the one the root service passed) skips the dialog.
    fn ensure_screen(&self) -> anyhow::Result<()> {
        let mut screen = self.screen.lock().unwrap();
        if screen.is_some() {
            return Ok(());
        }
        let store = self.token_file.clone().map(TokenStore::new);
        let token = store.as_ref().and_then(TokenStore::load).or_else(|| self.env_token.clone());
        let store_for_sink = store.clone();
        let opened = Screen::open(token, move |token| {
            if let Some(store) = store_for_sink.as_ref() {
                if let Err(e) = store.save(token) {
                    tracing::warn!("cannot store the screen cast restore token: {e:#}");
                }
            }
            // The helper process cannot write the state dir; the daemon persists what it reads from the wire.
            screencast::publish_token(token);
        })?;
        tracing::info!(
            "screen cast started: {} monitor(s), restore token {}",
            opened.displays()?.len(),
            if opened.restore_token().is_some() { "kept" } else { "not offered" }
        );
        *screen = Some(opened);
        Ok(())
    }
}
