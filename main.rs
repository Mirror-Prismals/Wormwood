fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(error) = wormwood::run_cli(&args) {
        eprintln!("Wormwood: {error}");
        std::process::exit(1);
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" { fn ww_game_restart_requested() -> bool; }
        if unsafe { ww_game_restart_requested() } {
            // Re-enter the same engine after normal world, audio and GPU cleanup.
            // exec preserves the launcher's child PID and its quit/stop controls.
            use std::os::unix::process::CommandExt;
            let error = std::env::current_exe().and_then(|path| {
                Err::<(),_>(std::process::Command::new(path).args(args).exec())
            }).unwrap_err();
            eprintln!("Could not restart Mirror: {error}");
            std::process::exit(1);
        }
    }
}
