//! `oc-pty` entry point: the thin binary wrapper around [`oc_pty::daemon`]
//! (design §2.2 — parse args, claim, serve; no terminal semantics here).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match oc_pty::parse_daemon_args(&args) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("oc-pty: {error}");
            std::process::exit(2);
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("oc-pty: tokio runtime");
    let code = runtime.block_on(oc_pty::daemon::run(config));
    // Exit codes (§5.1): 0 = shutdown/self-exit, 1 = claim/bind
    // failure (client retries in a fresh directory), 2 = usage.
    std::process::exit(code);
}
