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

- **Milestone 1 round 5 (fourth play-test requests).** 120 game tests + 23 ranked, clippy clean,
  autopilot match run with no errors.
  - Lob: leaves the hand 40% faster and the whole arc is flown 40% faster (same height). It
    re-solves its path every step, so it comes down on top of the target's head even if they
    run. Dashing at any moment during the flight shakes it off for good.
  - Curve: 40% faster again (bend kept the same). It bends out, then steers itself in during
    the last 0.35 s with guidance that always closes the gap; goes around cover as before.
    A dash shakes it off too. (Catching still works against both.)
  - Bodies: real joint chain (spine, neck, shoulders, elbows, wrists, hips, knees, ankles),
    hands with a palm, four two-jointed fingers and a thumb, rounded joint capsules connecting
    every segment, biceps/forearm/calf shapes, kneecaps, jaw, ears, nose, hair, shoes. Running
    lean, breathing, gripping the held ball, open hands to catch, fingers letting go on a throw.
  - Air spin (Q/E) turns only the body 180 degrees; the camera stays put. The body turns back
    to the camera once you move on the ground, charge or catch. (Catch uses the body's facing,
    so a spin can catch a ball coming from behind.)
  - Perfect catch (and a returning boomerang): the ball is already fully charged, so a single
    click throws it at full power immediately. The next ball picked up charges normally.

- **Milestone 2 - parkour (playable).** 132 game tests + 23 ranked, clippy clean. Verified in
  the real app with the `--drill` autopilot: offline and as a LAN client (7 full laps, no
  errors): wall run, wall jump, wall-to-wall, climb, ledge hang, throw while hanging, shimmy,
  climb up on top.
  - Every character, no mount. Logic in `parkour.rs` (pure, unit-tested on the real arena).
  - Wall run: jump next to a tall wall while moving along it: run along it up to 1.3 s with
    low gravity. Space = wall jump (up and away, keeps some speed; air jumps refilled).
    Each wall once per landing.
  - Wall-to-wall: wall jump across to another wall and keep moving along it to wall run again.
  - Wall climb: jump into a wall too tall to vault: run up it for 0.5 s.
  - Ledge hang: in the air near a ledge at hand height (falling, or at the top of a climb):
    hang. A/D shimmy (stops at the ends), Space climb up onto the top, S+Space jump off
    backward, Ctrl let go. Throw and catch still work (the body turns to aim one-handed).
  - Vault / wall mount: in the air pushing into a ledge up to chest height (low wall, small
    blocks): pull straight up onto it.
  - Arena: a 5.5 m parkour panel on each flank, 4.2 m from the side wall (corridor for
    wall-to-wall jumps). Pillars and blocks can be climbed and hung from too.
  - Animations: lean off the wall while wall running, hand-over-hand climb, two-hand hang
    (one hand while throwing), pull-up over the ledge. Other players see them (protocol 5).
  - HUD hints while hanging / wall running; controls line added.
  - Being slimed, bricked, launched or dashing ends any parkour move.

- **Milestone 2 follow-ups (play-test requests).** 158 game tests + 23 ranked, clippy clean.
  Verified in the real app: classic and grand arenas offline with 3 bots on autopilot (cover
  throws, parkour, catches), and a LAN host + drilling client (8 parkour laps, 4 throws
  from a hang, no errors). Protocol 6.
  - Hanging with a ball: you pull up so your head looks over the ledge, one hand flat on
    it, the ball cocked back behind your head (thrown from over the ledge). Lock-on still
    only while charging.
  - Lock-on: starting a charge locks the enemy under the crosshair (else the nearest).
    While charging, swinging the mouse toward another enemy switches to them (needs to be
    clearly closer to the crosshair, so it doesn't flicker); the camera stops pulling
    toward the target for 0.35 s after the mouse moves. Tab/MMB still cycle.
  - Throw animation: torso coils back on the wind-up and whips through, free arm pulls
    back, a step into the throw with the hips driving forward; small camera kick.
  - Catch animation: the impact shoves the catcher back (push scaled by the ball's speed,
    bots too), the body rocks back and both arms wrap the ball into the chest, hunched over
    it, then ease back; a shockwave ring (gold for a perfect catch) and a camera jolt.
  - Smoother movement: any wall move (wall run, wall jump, climb, hang, vault, climb up)
    gives back the double jump and both dashes at once; an air dash carries its momentum
    into the fall; jump buffering (0.12 s before landing) and coyote time (0.12 s after
    walking off a ledge); the body turns smoothly to walls instead of snapping.
  - Grand arena (lobby key V cycles classic day / classic night / grand day / grand
    night, or `--grand`): 80 x 80 m, 10 m walls, 34 mirrored pieces of cover: low walls,
    pillars, crates with step blocks, L-shaped bunkers, a 4 m wall-run corridor, four side
    parkour panels with wall-to-wall corridors, two centre panels, long ledge walls.
  - Cover (everyone): F next to a wall at least 1 m high takes cover: crouched against it
    (a 1.1 m target, hidden behind the 1.2 m low walls), A/D slides along it. Hold LMB to
    pre-charge with lock-on while hidden; you rise up and the throw goes over a low wall
    (tilted up just enough to clear it), or curves around the nearer end of a tall one.
    You duck again 0.45 s after throwing. F again, sprinting, jumping or dashing leaves.
    F while charging is still a lob; F away from walls with a ball also lobs.
  - Detail: procedural textures with normal maps and mipmaps (no image files): wooden
    court floor with painted lines, centre circle and pad rings; brick low walls with
    concrete caps; concrete blocks with lips; painted metal parkour panels with seams,
    rivets and a dark grab stripe; glass outer walls in a metal frame (posts, top rail,
    kick plate). Bloom on glowing things. Players: woven fabric clothes, soft skin
    texture, collar, chest stripe, sleeve cuffs, wristbands, belt with buckle, white soles,
    toe caps, sock cuffs, eyebrows, chin, glossy visor.
  - Balls: smooth high-res spheres with a pebbled grip and seams (the boomerang is a ball
    now, with a spinning band). Only while flying they turn fluid: stretched along the
    path (volume kept), jiggling like jelly, spinning about the flight line, with
    swirling wind streaks and a fading trail. Round again at rest and in hand. Held balls
    sit in the holder's palm (cradled in both hands right after a catch). The brick is
    unchanged.

- **Milestone 3 - mounts (playable).** 183 game tests + 23 ranked, clippy clean. Verified in
  the real app with the `--ride-drill` autopilot: offline, every mount completes a lap (ollie
  onto the rail holding F, grind it, flip trick off the end and land; back flips on alternate
  laps), and as a LAN client (6 mounts, the host sees the rider, mount and combos). Protocol 7.
  - Six mounts, each in its own RON data file in `crates/game/data/mounts/`: skateboard,
    longboard, BMX, inline skates, scooter, soap shoes. Each has 10 flip tricks, 10 grabs,
    10 grinds, plus a front flip and a back flip; soap shoes take every inline skate trick
    (`tricks_from: Some(InlineSkates)`) with their own handling. The longboard is a
    stretched skateboard: faster, slower to turn, less pop, slower-flipping deck.
  - Each trick's file entry gives its name, keys, duration, points and animation (board
    roll / spin / pitch, rider spin / flip, barspin, tailwhip, which hand grabs where, leg
    shape, lean; grinds: board angle, tilt, crouch). The files are built in; a
    `data/mounts` folder next to where the game starts overrides them (bad files fall back
    to the built-in ones with a log message saying why).
  - Riding: 1-6 hop on a mount (same key or X hops off; getting slimed, bricked, launched or
    knocked out knocks you off). WASD carves toward that direction and pushes (Shift pushes
    harder), pulling back brakes, letting go coasts; you glance off walls. Space ollies.
    In the air: Q + WASD (+Shift) flip trick, E + WASD grab (hold E for bonus points),
    R + W / R + S front / back flip. Hold F coming down onto a rail or the top of a low
    wall to grind (F + WASD picks the grind); Space jumps off, the end pops you up.
    Throwing, catching and dashing all still work while riding.
  - Combos: tricks chain until you land; landing scores the sum x the number of tricks
    (max x10). Landing a flip before 80% of it is done is a bail (off balance 1 s, combo
    lost). Combos of 500+ show in everyone's feed; 1000+ while holding a ball charges it
    (next throw at full power).
  - Grind rails: free-standing rails in both arenas (2 classic, 6 grand), and the tops of
    every low wall are grindable ledges.
  - Visuals: mount models (deck with grip tape, trucks and rolling wheels; longboard; BMX
    with frame, spoked wheels, pegs, cranks, spinning bars; scooter with stem and bars;
    inline skates with four wheels; soap shoes with sole plates). Rider stances: side-on
    on boards, up on the pedals over the bars on the BMX, kicking on the scooter, skating
    stride on skates. Tricks animate from the data (kickflips roll the board, shove-its
    spin it, tailwhips whip the frame round the head tube while the bars stay, barspins,
    grabs reach for the board, flips go round with the mount). Grinds set the board at
    the grind's angle. Bails windmill the arms.
  - HUD: mount, speed, live combo and trick feed on the left; H hides the controls list.

## How to run
- Host (with 1 bot): `cargo run -p brawl_game`
- Join from another PC on the LAN: `cargo run -p brawl_game -- --join <host-ip> --name Ana`
- Offline practice: `cargo run -p brawl_game -- --offline --bots 3`
- Host keys in warm-up: Enter start, B add bot, N remove bot, M mode, V map. After a match:
  Enter back to warm-up.
- Esc frees the mouse; click to capture it again.
- Host keys in warm-up also: V cycles the map (classic/grand, day/night). `--grand` starts
  on the grand arena.
- Mounts: 1 skateboard, 2 longboard, 3 BMX, 4 inline skates, 5 scooter, 6 soap shoes, X off.
  Edit tricks in `crates/game/data/mounts/*.ron` (rebuild), or copy that folder to
  `data/mounts` next to where you start the game to change them without rebuilding.
- Testing aids: `--ride-drill` (autopilot rides every mount: grinds, flip tricks, back flips),
  `--autopilot` (bot-driven local player, never grabs the mouse; host auto-starts;
  takes cover and throws over walls every fourth throw),
  `--drill` (autopilot loops the parkour drill on the +X flank),
  `--bind 127.0.0.1` (host on loopback only, no firewall prompt).

## In progress
Nothing. Milestone 3 waiting for a human play-test of the feel (speeds, turning, trick
timing, how easy grinds are to catch).

## Known bugs / limits
- Movement is client-authoritative (fine for LAN with friends, not cheat-proof).
- Camera can clip into cover blocks (it is only kept inside the outer walls).
- Host-based P2P: if the host quits, the match ends for everyone.
- The kill feed may miss an event if a UDP packet is lost (scores/state are unaffected).
- First LAN host run triggers a Windows Firewall prompt; allow it or peers can't join.
- No sound, no player name tags yet.
- Bots don't do parkour yet (they still play the ball game normally around the panels).
- Parkour uses the arena's box list, so it only works on axis-aligned boxes (all current
  geometry); corners can't be wall run around.
- Bots don't take cover. Remote players' charge wind-up isn't sent over the network (they
  are seen holding, then throwing).
- Taking cover behind a wall taller than 2.6 m: throws curve round its nearer end; there
  is no lean-out-sideways pose yet.
- Bots don't ride mounts. There are no ramps or quarter pipes yet (Milestone 4 adds a skate
  park), so the longest tricks (0.8 s+) need the extra air from popping off a rail or
  dropping off a wall.
- Mount models are built from simple shapes; the rider's hands and feet line up with the
  bars, deck and pedals only roughly.

## Next step
Milestone 4: community map loader (folder plus manifest); all base maps get skateable
walls, grind rails and an open skate park.
