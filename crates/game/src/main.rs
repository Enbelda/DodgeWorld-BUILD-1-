//! Original team dodgeball brawler - game client entry point.

fn main() {
    let cfg = match brawl_game::config::parse(std::env::args().skip(1)) {
        Ok(cfg) => cfg,
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(2);
        }
    };
    if let Err(msg) = brawl_game::run(cfg) {
        eprintln!("error: {msg}");
        std::process::exit(1);
    }
}
