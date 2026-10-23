fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = wormwood::run_cli(&args) {
        eprintln!("Wormwood: {error}");
        std::process::exit(1);
    }
}
