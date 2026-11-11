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

- **Milestone 1 round 3 (second play-test requests).** 89 game tests + 23 ranked, clippy clean.
  Verified in LAN autopilot matches: night map + standard, and day + deathmatch to a win
  (bricks, slime, catches, boomerang, double-jump flips and air spins on both machines).
  Supersedes some round-2 choices: catch is a timed press again; powered = double damage.
  - Humanoid models (`rig.rs`): torso, head + visor, two-part arms with hands, two-part legs
    with feet. Animated: walk cycle, ball held out front, wind-up while charging, throw swing,
    arms reach forward while catching, spread-eagle when slimed, flat on the back when stunned,
    tucked flips. White flash on every hit.
  - Health: 4 hearts (8 halves). Normal/slime/boomerang hit = half a heart, powered = 1 heart,
    brick = 2 hearts (powered brick = 4). HUD hearts drawn from shapes (no font needed).
  - Curve and lob: curved/lobbed throws 30% faster, bend/arc 40% stronger (curve accel scaled
    so the visible bend really grows 40% at the higher speed). Walls raised to 8 m for lobs.
  - Brick (red block, centre pad): 2 hearts + knocked flat 5 s (no move/throw/catch, drops ball).
  - Catch: RMB opens a 0.4 s window (time it); 1 s cooldown after a miss.
  - Lock-on: starting a charge auto-locks the nearest living enemy (Tab still toggles manually).
  - Deathmatch mode (lobby key M or `--deathmatch`): no respawns, last team standing wins.
  - Night map (lobby key V or `--night`): dark sky, moon, 450 stars, 10 street lights.
  - Double jump does a front flip (or back flip when moving backward).
  - Air tricks: Q/E = 180 spin left/right (really turns you), W+Q/E front flip, S+Q/E back flip.
    Q/E still curve while charging a throw.
  - Protocol 3. Bots-only standard match now ~5.5 min (was ~2 with 2 HP).

- **Milestone 1 round 4 (third play-test requests).** 112 game tests + 23 ranked, clippy clean.
  Verified in LAN autopilot matches: perfect catches, ricochet knockbacks, tackles (incl. by the
  joining client), dashes, slime, bricks, boomerangs, flips and spins; no errors.
  - Lob solves its arc to come down on the top of the target's head (lock, else nearest enemy
    to the aim).
  - Catching: no damage from any ball while the catch window is open (only a ball you face is
    caught; others bounce off).
  - Slimed players are helpless: no throw, catch or turning (stunned/launched too).
  - Dash (Ctrl): ground or air, 6 m in 0.12 s, 2 charges, 0.2 s between, both back 1.5 s after
    the last dash. Invincible + glowing blue while dashing. Dashing into an enemy tackles them:
    launched into the air (host flies them until they land) and their ball pops up.
  - Curved throws at someone behind cover swing around it (side = the curve's bulge) and then
    track the target.
  - Perfect catch: a flashing red ring around the crosshair while catching now would be perfect
    (ball < 0.15 s away). Perfect catch = powered + fully charged + one speed stack. Stacks make
    the thrown ball x2/x3/x4/x5 base speed (max 4 stacks; literal doubling would reach 16x).
  - Lock-on is only on while charging (nearest enemy); Tab/MMB while charging switches target.
  - Wall ricochet: a live ball hitting a wall bounces back at double speed; if it hits anyone
    it knocks them back (no damage, no points). Boomerangs still return instead.
  - Sprint (Shift) 1.4x; a jump taken while sprinting carries 1.24x farther, dashes too.
  - Protocol 4.

## How to run
- Host (with 1 bot): `cargo run -p brawl_game`
- Join from another PC on the LAN: `cargo run -p brawl_game -- --join <host-ip> --name Ana`
- Offline practice: `cargo run -p brawl_game -- --offline --bots 3`
- Host keys in warm-up: Enter start, B add bot, N remove bot, M mode, V map. After a match:
  Enter back to warm-up.
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
