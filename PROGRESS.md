# Progress

## Done
- First task: `brawl_ranked` tests pass (23/23). Fixed its one clippy warning (derive `Default` for `RankState`).
- Cargo workspace: `crates/brawl_ranked` + `crates/game` (`brawl_game`, Bevy 0.20 + bevy_rapier3d 0.37, depends on `brawl_ranked`).
- **Milestone 1 - core loop (playable).** Build, 44 game tests + 23 ranked tests, clippy (-D warnings) all clean.
  Play-tested in the real app: offline with 3 bots, and a LAN host + joining client on autopilot
  played a full match to 10 (throws of every kind, hits, knockouts, respawns, catches incl. the
  client's, end screen).
  - Movement: WASD, jump, Rapier character controller, over-the-shoulder mouse-look camera.
  - Balls: pick up by walking over; hold LMB to charge, release to throw; Q/E curve; F lob;
    RMB catch (0.4 s window, must face the ball, 1 s cooldown). A caught ball is powered (one-hit KO).
  - Knockouts: 2 HP, KO -> 3 s respawn, team scores; first to 10. Warm-up lobby (KOs don't count).
  - Bots (fetch, strafe, throw/curve/lob, sometimes catch, steer around cover).
  - LAN lobby for 2-4 players over UDP: one peer hosts the match, others join. Actions resent
    until acked; timeouts; full-lobby/version rejection.

## How to run
- Host (with 1 bot): `cargo run -p brawl_game`
- Join from another PC on the LAN: `cargo run -p brawl_game -- --join <host-ip> --name Ana`
- Offline practice: `cargo run -p brawl_game -- --offline --bots 3`
- Host keys in warm-up: Enter start, B add bot, N remove bot. After a match: Enter back to warm-up.
- Esc frees the mouse; click to capture it again.
- Testing aids: `--autopilot` (bot-driven local player, never grabs the mouse; host auto-starts),
  `--bind 127.0.0.1` (host on loopback only, no firewall prompt).

## In progress
Nothing. Milestone 1 waiting for a human play-test of the feel (mouse aim, throw/catch timing).

## Known bugs / limits
- Movement is client-authoritative (fine for LAN with friends, not cheat-proof).
- Camera can clip into cover blocks (it is only kept inside the outer walls).
- Host-based P2P: if the host quits, the match ends for everyone.
- The kill feed may miss an event if a UDP packet is lost (scores/state are unaffected).
- First LAN host run triggers a Windows Firewall prompt; allow it or peers can't join.
- No sound, no player name tags yet.

## Next step
Milestone 2: parkour (wallrun, wall mount, wall-to-wall jump, ledge hang, throw while hanging).
