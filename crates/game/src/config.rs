//! Command-line launch options.

use crate::net::DEFAULT_PORT;
use crate::sim::MAX_PLAYERS;

pub const USAGE: &str = "\
usage: brawl_game [options]
  (no options)       host a LAN lobby with 1 bot
  --join ADDR[:PORT] join a LAN host
  --offline          host without opening a network port
  --bots N           bots to add when hosting (0-3, default 1)
  --port PORT        host port (default 47800)
  --name NAME        your display name
  --bind IP          host on this local address only (e.g. 127.0.0.1)
  --autopilot        bot-driven local player, for automated play-tests";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Host { lan: bool },
    Join(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchConfig {
    pub mode: Mode,
    pub name: String,
    pub port: u16,
    pub bots: usize,
    /// Host address to listen on; `None` = all interfaces.
    pub bind: Option<std::net::IpAddr>,
    pub autopilot: bool,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        LaunchConfig { mode: Mode::Host { lan: true }, name: "Player".into(), port: DEFAULT_PORT, bots: 1, bind: None, autopilot: false }
    }
}

/// Parse arguments (without the program name).
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<LaunchConfig, String> {
    let mut cfg = LaunchConfig::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| it.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--join" => cfg.mode = Mode::Join(value("--join")?),
            "--offline" => cfg.mode = Mode::Host { lan: false },
            "--name" => cfg.name = value("--name")?,
            "--port" => {
                let v = value("--port")?;
                cfg.port = v.parse().map_err(|_| format!("bad port: {v}"))?;
            }
            "--bots" => {
                let v = value("--bots")?;
                let n: usize = v.parse().map_err(|_| format!("bad bot count: {v}"))?;
                if n >= MAX_PLAYERS {
                    return Err(format!("at most {} bots", MAX_PLAYERS - 1));
                }
                cfg.bots = n;
            }
            "--autopilot" => cfg.autopilot = true,
            "--bind" => {
                let v = value("--bind")?;
                cfg.bind = Some(v.parse().map_err(|_| format!("bad address: {v}"))?);
            }
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<LaunchConfig, String> {
        parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn defaults_to_lan_host_with_one_bot() {
        assert_eq!(p("").unwrap(), LaunchConfig::default());
    }

    #[test]
    fn join_with_name() {
        let c = p("--join 192.168.1.4 --name Ana").unwrap();
        assert_eq!(c.mode, Mode::Join("192.168.1.4".into()));
        assert_eq!(c.name, "Ana");
    }

    #[test]
    fn offline_bots_and_port() {
        let c = p("--offline --bots 3 --port 5000").unwrap();
        assert_eq!(c.mode, Mode::Host { lan: false });
        assert_eq!((c.bots, c.port), (3, 5000));
    }

    #[test]
    fn bind_and_autopilot() {
        let c = p("--bind 127.0.0.1 --autopilot").unwrap();
        assert_eq!(c.bind, Some([127, 0, 0, 1].into()));
        assert!(c.autopilot);
        assert!(p("--bind nope").is_err());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(p("--bots 4").is_err());
        assert!(p("--port nope").is_err());
        assert!(p("--join").is_err());
        assert!(p("--wat").is_err());
    }
}
