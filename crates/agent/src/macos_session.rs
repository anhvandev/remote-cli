//! macOS session helpers (ticket 24): the wire and daemon-side platform live in `session`, shared with Linux
//! (ticket 16); this module keeps the macOS name the CLI and the LaunchAgent use.

pub use crate::session::{answer, run_helper, SessionPlatform, LOGIN_WINDOW_INPUT};
