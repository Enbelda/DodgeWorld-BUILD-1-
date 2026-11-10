# Project: original team dodgeball brawler (Rust, Bevy, Rapier)

Original game. Do not copy any existing game's characters, maps, art, names or code.
The crate `brawl_ranked` (ranked ladder and matchmaking) already exists in this folder.

## Rules of work
- Work one milestone at a time, in order.
- After each milestone: cargo build, cargo test, cargo clippy, run the game, fix bugs, then git commit.
- Keep PROGRESS.md updated with: done, in progress, known bugs, next step.
- Never mark a milestone done until it compiles and runs.
- Work unattended as far as possible. Stop and tell the user only when a milestone is playable or you are blocked.

## First task
Run `cargo test` in brawl_ranked and fix any failures. Then turn this folder into a Cargo workspace with a Bevy game crate that depends on brawl_ranked, and begin milestone 1.

## Milestones
1. Core loop: movement, ball throw/catch/curve/lob, knockouts, local P2P lobby for 2-4 players.
2. Parkour (all characters, no mount needed): wallrun, wall mount, wall-to-wall jump, ledge hang, throw while hanging.
3. Mounts: skateboard (10 grab, 10 flip, 10 grind tricks plus back/front flip), BMX (10 of each), inline skates (10 of each), longboard (similar to skateboard, adapted to its size), scooter (10 flip, grab, grind), soap shoes (all inline skate tricks). Tricks defined in RON data files.
4. Maps: community map loader (folder plus manifest). All base maps have skateable walls, grind rails and an open skate park.
5. Settings: FOV, reticle customization, UI color scheme, telemetry toggles. Cosmetics get per-slot color customization (e.g. shirt color 1 torso, 2 arms, 3 design).
6. Hideout: host-only custom match menu in the pause menu: 1v1, 2v2, 3v3, handicap (1v2, 1v3...), red light green light.
7. Ranked queues 4v4, 5v5, 6v6 using the brawl_ranked crate.
8. Invincibility power-up: one per game at 10 minutes, 30-second big alert visible through walls with a gold outline, 10 seconds of invincibility to everything except falling off the map.
9. Voice chat: private team channel and open proximity channel that enemies can hear.
