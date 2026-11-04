#[tokio::main]
async fn main() {
    if let Err(e) = rcli::run(std::env::args_os(), &mut std::io::stdout()).await {
        if let Some(exit) = e.downcast_ref::<rcli::RemoteExit>() {
            if exit.timed_out {
                eprintln!("error: {exit}");
            }
            std::process::exit(exit.exit_code());
        }
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
