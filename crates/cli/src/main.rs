#[tokio::main]
async fn main() {
    if let Err(e) = rcli::run(std::env::args_os(), &mut std::io::stdout()).await {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
