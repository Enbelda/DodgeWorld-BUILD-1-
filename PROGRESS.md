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

- **Milestone 1 additions (requested after play-test).** 67 game tests + 23 ranked, clippy clean,
  verified in a LAN autopilot match (locks, catches, boomerang returns, slime incl. on the client).
  - Lock-on: Tab / middle mouse locks the enemy nearest the crosshair (40 m, 35 deg cone).
    Camera tracks the target, throws home in (1.2 rad/s turn). Red ring over your target;
    "LOCKED ON BY ..." warning for whoever is targeted. Bots lock on for ~35% of throws.
  - Incoming-throw indicator: red "!" around the crosshair points at the closest enemy ball
    flying at you (or homing on you), plus an INCOMING alert.
  - Catch: hold RMB; the catch fires the moment a ball arrives (still must face it).
    Moving is 40% slower while holding catch.
  - Curve and lob 30% stronger (curve bend x1.3, lob peak height x1.3).
  - Boomerang ball (purple disc): after hitting an opponent or any surface it flies back to the
    thrower and arrives charged (next throw = full charge + speed bonus, normal damage).
  - Slime ball (green blob): a hit does 1 damage, drags the target to the nearest wall/cover and
    sticks them there (3 s total). They can still turn, throw and catch.
  - 6 balls: 4 normal + boomerang and slime on the centre line (x = -13 / +13).
  - Double jump (one extra jump in the air).
  - Protocol bumped to 2 (old builds can't join new hosts).

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
