//! The Windows service host (ticket 22): `sc.exe`/the SCM start `rcli.exe agent service --config …`, which hands
//! the process to the `windows-service` dispatcher. The dispatcher owns the SCM status (start pending → running →
//! stopped), so the process reports its state instead of dying with error 1053 ("did not respond in a timely
//! fashion"). All the Agent work happens on a worker thread with its own tokio runtime; the service thread only
//! answers control events.
//!
//! The service config file is read here, not from the command line, so the Server Key never appears in `sc qc` or
//! the registry. Session helpers are launched by `rcli_agent::windows_session::watch_helpers`.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

use windows_service::{
    define_windows_service,
    service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType},
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use crate::windows_install::{ServiceConfig, SERVICE};

define_windows_service!(ffi_service_main, service_main);

/// The SCM entry point. A service the SCM did not start runs the same thing in the foreground instead, which makes
/// the service host debuggable from a shell.
pub fn run(config: &Path) -> anyhow::Result<()> {
    match service_dispatcher::start(SERVICE, ffi_service_main) {
        Ok(()) => Ok(()),
        Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1063) => {
            // ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: started from a shell, not by the SCM.
            tracing::warn!("not started by the Service Manager: running the service in the foreground");
            let args: Vec<OsString> = std::env::args_os().skip(1).collect();
            let _ = config;
            service_main(args);
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Called by the dispatcher with the arguments the SCM passed (`agent service --config …`).
fn service_main(args: Vec<OsString>) {
    if let Err(e) = serve(args) {
        tracing::error!("the service stopped: {e:#}");
    }
}

fn serve(args: Vec<OsString>) -> anyhow::Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    let handler = {
        let stop = stop.clone();
        service_control_handler::register(SERVICE, move |control| match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                stop.store(true, Ordering::SeqCst);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })?
    };
    let status = |state, checkpoint, wait: u64, exit| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
        exit_code: exit,
        checkpoint,
        wait_hint: Duration::from_secs(wait),
        process_id: None,
    };
    // Report early: the SCM gives a service only a few seconds before it calls it hung.
    handler.set_service_status(status(ServiceState::StartPending, 1, 10, ServiceExitCode::NO_ERROR))?;

    let config_path = config_arg(&args).ok_or_else(|| anyhow::anyhow!("the service needs --config <path>"))?;
    let config = ServiceConfig::load(&config_path)?;
    init_log(&config.state_dir);

    rcli_agent::windows_session::watch_helpers(std::env::current_exe()?, config.pipe.clone());
    let (result_tx, result_rx) = mpsc::channel::<anyhow::Result<()>>();
    let device = crate::agent::DeviceConfig {
        server: config.server.clone(),
        key: config.key.clone(),
        state_dir: config.state_dir.clone(),
        name: config.name.clone(),
        socket: rcli_agent::ipc::default_socket(),
        session_socket: None,
        session_pipe: Some(config.pipe.clone()),
    };
    std::thread::spawn(move || {
        let result = tokio::runtime::Runtime::new()
            .map_err(anyhow::Error::from)
            .and_then(|runtime| runtime.block_on(crate::agent::serve_device(device)));
        let _ = result_tx.send(result);
    });
    handler.set_service_status(status(ServiceState::Running, 0, 0, ServiceExitCode::NO_ERROR))?;

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match result_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Ok(())) => break,
            Ok(Err(e)) => {
                tracing::error!("the Agent stopped: {e:#}");
                handler.set_service_status(status(ServiceState::Stopped, 0, 0, ServiceExitCode::ServiceSpecific(1)))?;
                return Ok(());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // shortcut: no graceful drain; ending the process drops the Server connection (it reconnects after a restart).
    handler.set_service_status(status(ServiceState::Stopped, 0, 0, ServiceExitCode::NO_ERROR))?;
    Ok(())
}

/// The `--config` value the SCM passed, either as two arguments or as `--config=…`.
fn config_arg(args: &[OsString]) -> Option<PathBuf> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
        if let Some(path) = arg.to_str().and_then(|a| a.strip_prefix("--config=")) {
            return Some(PathBuf::from(path));
        }
    }
    None
}

/// The service has no console: log beside the Device's data, where an Admin already looks.
fn init_log(state_dir: &Path) {
    let _ = std::fs::create_dir_all(state_dir);
    let path = state_dir.join("agent.log");
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(move || {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .unwrap_or_else(|_| std::fs::File::open("NUL").expect("NUL exists on Windows"))
        })
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_config_argument_is_read_from_the_arguments_the_scm_passes() {
        assert_eq!(
            config_arg(&["agent".into(), "service".into(), "--config".into(), r"C:\ProgramData\rcli\service.json".into()]),
            Some(PathBuf::from(r"C:\ProgramData\rcli\service.json"))
        );
        assert_eq!(config_arg(&[OsString::from(r"--config=C:\x\service.json")]), Some(PathBuf::from(r"C:\x\service.json")));
        assert_eq!(config_arg(&["agent".into()]), None);
    }
}
