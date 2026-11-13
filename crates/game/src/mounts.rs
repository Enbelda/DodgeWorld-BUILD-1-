//! Mounts and their tricks, defined in RON data files (`crates/game/data/mounts/*.ron`).
//!
//! Six mounts: skateboard, longboard, BMX, inline skates, scooter and soap shoes. Each file
//! gives the mount's handling and its tricks: 10 flip tricks, 10 grabs, 10 grinds, plus a
//! front and a back flip. Soap shoes borrow every inline skate trick (`tricks_from`).
//!
//! The files are built into the game, so it runs from anywhere. A `data/mounts` folder
//! next to where the game is started overrides them (edit tricks without recompiling).

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum MountKind {
    Skateboard,
    Longboard,
    Bmx,
    InlineSkates,
    Scooter,
    SoapShoes,
}

impl MountKind {
    pub const ALL: [MountKind; 6] = [
        MountKind::Skateboard,
        MountKind::Longboard,
        MountKind::Bmx,
        MountKind::InlineSkates,
        MountKind::Scooter,
        MountKind::SoapShoes,
    ];

    /// The number key (1-6) that picks this mount.
    pub fn key(self) -> u8 {
        MountKind::ALL.iter().position(|k| *k == self).unwrap_or(0) as u8 + 1
    }

    /// Worn on the feet (skates, soap shoes) rather than stood or sat on.
    pub fn on_feet(self) -> bool {
        matches!(self, MountKind::InlineSkates | MountKind::SoapShoes)
    }
}

/// How the rider stands.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stance {
    /// Side-on to the direction of travel (boards).
    Sideways,
    /// Facing forward, hands on the bars (BMX, scooter).
    Forward,
    /// Skating stride (skates, soap shoes).
    Skates,
}

/// Which WASD keys are held when the trick button is pressed.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum Dir {
    #[default]
    None,
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl Dir {
    /// From the keys held: W, S, A, D.
    pub fn from_keys(up: bool, down: bool, left: bool, right: bool) -> Dir {
        let v = (up as i8) - (down as i8);
        let h = (right as i8) - (left as i8);
        match (v, h) {
            (1, -1) => Dir::UpLeft,
            (1, 1) => Dir::UpRight,
            (-1, -1) => Dir::DownLeft,
            (-1, 1) => Dir::DownRight,
            (1, _) => Dir::Up,
            (-1, _) => Dir::Down,
            (_, -1) => Dir::Left,
            (_, 1) => Dir::Right,
            _ => Dir::None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub struct TrickInput {
    pub dir: Dir,
    #[serde(default)]
    pub shift: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Hand {
    Left,
    #[default]
    Right,
    Both,
}

/// Where a grab takes hold.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Spot {
    Nose,
    Tail,
    Toe,
    Heel,
    #[default]
    Mid,
    /// Handlebars (or letting go of them).
    Bars,
    Seat,
    /// A skate / foot (or nothing: arms out).
    Skate,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Grab {
    pub hand: Hand,
    pub spot: Spot,
}

/// What the legs do during a trick.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Legs {
    #[default]
    Normal,
    /// Knees to the chest.
    Tuck,
    /// Legs straight out.
    Extend,
    /// One leg kicked out.
    Kick,
    /// One foot off.
    OneFoot,
    /// Legs apart.
    Split,
    /// Legs crossed.
    Cross,
}

/// How a trick looks. Revolutions for the spins and flips, degrees for grind angles.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
#[serde(default)]
pub struct TrickAnim {
    /// Board / skates roll about the travel axis (kickflip).
    pub board_roll: f32,
    /// Board spins flat about the up axis (shove-it).
    pub board_spin: f32,
    /// Board flips nose over tail (impossible).
    pub board_pitch: f32,
    /// Rider spins about the up axis (360).
    pub body_spin: f32,
    /// Rider flips: -1 front flip, +1 back flip.
    pub body_flip: f32,
    /// Handlebars spin (BMX, scooter).
    pub bar_spin: f32,
    /// Frame / deck whips round the head tube (tailwhip).
    pub frame_whip: f32,
    pub grab: Option<Grab>,
    pub legs: Legs,
    /// Body lean (+ back, - forward).
    pub lean: f32,
    /// Grind: board angle to the rail (degrees; 90 = across it).
    pub angle: f32,
    /// Grind: nose up (+) / down (-) (degrees).
    pub tilt: f32,
    /// Grind: how low the rider crouches (0..1).
    pub crouch: f32,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Trick {
    pub name: String,
    pub input: TrickInput,
    /// How long the trick takes (air tricks). Landing before 80% of it is a bail.
    #[serde(default = "default_secs")]
    pub secs: f32,
    /// Points for landing it (grinds: per second on the rail).
    pub points: u32,
    #[serde(default)]
    pub anim: TrickAnim,
}

fn default_secs() -> f32 {
    0.5
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct Stats {
    /// Cruising top speed (m/s); pushing with Shift goes 20% faster.
    pub top_speed: f32,
    /// Acceleration while pushing (m/s^2).
    pub push: f32,
    /// Turn rate (rad/s) at low speed.
    pub turn: f32,
    /// Ollie / bunny hop up speed (m/s).
    pub jump: f32,
    /// Rolling resistance when coasting (fraction of speed lost per second).
    pub friction: f32,
    /// Speed along a rail.
    pub grind_speed: f32,
}

/// The four kinds of trick.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum TrickCat {
    Flip,
    Grab,
    Grind,
    /// Front / back flip of the whole rider.
    AirFlip,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct MountDef {
    pub name: String,
    pub kind: MountKind,
    pub stance: Stance,
    pub stats: Stats,
    #[serde(default)]
    pub flips: Vec<Trick>,
    #[serde(default)]
    pub grabs: Vec<Trick>,
    #[serde(default)]
    pub grinds: Vec<Trick>,
    #[serde(default)]
    pub air_flips: Vec<Trick>,
    /// Use another mount's tricks (soap shoes do every inline skate trick).
    #[serde(default)]
    pub tricks_from: Option<MountKind>,
}

impl MountDef {
    pub fn tricks(&self, cat: TrickCat) -> &[Trick] {
        match cat {
            TrickCat::Flip => &self.flips,
            TrickCat::Grab => &self.grabs,
            TrickCat::Grind => &self.grinds,
            TrickCat::AirFlip => &self.air_flips,
        }
    }

    pub fn trick(&self, cat: TrickCat, index: usize) -> Option<&Trick> {
        self.tricks(cat).get(index)
    }

    /// The trick for these keys: an exact match, else the one with no direction.
    pub fn find(&self, cat: TrickCat, input: TrickInput) -> Option<usize> {
        let list = self.tricks(cat);
        list.iter()
            .position(|t| t.input == input)
            .or_else(|| list.iter().position(|t| t.input == TrickInput { dir: input.dir, shift: false }))
            .or_else(|| list.iter().position(|t| t.input == TrickInput::default()))
    }
}

/// Every mount, ready to use.
#[derive(bevy::prelude::Resource, Clone, Debug)]
pub struct MountLibrary {
    pub defs: Vec<MountDef>,
}

/// The built-in data files, in `MountKind::ALL` order.
pub const BUILT_IN: [(&str, &str); 6] = [
    ("skateboard.ron", include_str!("../data/mounts/skateboard.ron")),
    ("longboard.ron", include_str!("../data/mounts/longboard.ron")),
    ("bmx.ron", include_str!("../data/mounts/bmx.ron")),
    ("inline_skates.ron", include_str!("../data/mounts/inline_skates.ron")),
    ("scooter.ron", include_str!("../data/mounts/scooter.ron")),
    ("soap_shoes.ron", include_str!("../data/mounts/soap_shoes.ron")),
];

impl MountLibrary {
    /// Parse a set of (file name, RON text), fill in borrowed tricks and check it.
    pub fn parse(files: &[(&str, String)]) -> Result<MountLibrary, String> {
        let mut defs = Vec::new();
        for (name, text) in files {
            let def: MountDef = ron::from_str(text).map_err(|e| format!("{name}: {e}"))?;
            defs.push(def);
        }
        for kind in MountKind::ALL {
            if !defs.iter().any(|d| d.kind == kind) {
                return Err(format!("no data file for {kind:?}"));
            }
        }
        let snapshot = defs.clone();
        for d in &mut defs {
            if let Some(from) = d.tricks_from {
                let src = snapshot.iter().find(|s| s.kind == from).ok_or(format!("{}: tricks_from {from:?} missing", d.name))?;
                d.flips = src.flips.clone();
                d.grabs = src.grabs.clone();
                d.grinds = src.grinds.clone();
                d.air_flips = src.air_flips.clone();
            }
        }
        let lib = MountLibrary { defs };
        lib.check()?;
        Ok(lib)
    }

    /// The built-in mounts.
    pub fn built_in() -> MountLibrary {
        let files: Vec<(&str, String)> = BUILT_IN.iter().map(|(n, t)| (*n, t.to_string())).collect();
        MountLibrary::parse(&files).expect("built-in mount data is valid")
    }

    /// Mounts from `dir` if it holds a full, valid set; otherwise the built-in ones.
    pub fn load(dir: &std::path::Path) -> (MountLibrary, String) {
        let read: Result<Vec<(&str, String)>, String> = BUILT_IN
            .iter()
            .map(|(n, _)| std::fs::read_to_string(dir.join(n)).map(|t| (*n, t)).map_err(|e| format!("{n}: {e}")))
            .collect();
        match read.and_then(|files| MountLibrary::parse(&files)) {
            Ok(lib) => (lib, format!("mount data loaded from {}", dir.display())),
            Err(e) if dir.exists() => (MountLibrary::built_in(), format!("mount data in {} not used ({e})", dir.display())),
            Err(_) => (MountLibrary::built_in(), "built-in mount data".to_string()),
        }
    }

    pub fn get(&self, kind: MountKind) -> &MountDef {
        self.defs.iter().find(|d| d.kind == kind).unwrap_or(&self.defs[0])
    }

    /// Every mount has 10 flips, 10 grabs, 10 grinds, a front and a back flip, with no
    /// two tricks of a kind sharing the same keys.
    pub fn check(&self) -> Result<(), String> {
        for d in &self.defs {
            for (cat, want) in [(TrickCat::Flip, 10), (TrickCat::Grab, 10), (TrickCat::Grind, 10), (TrickCat::AirFlip, 2)] {
                let list = d.tricks(cat);
                if list.len() != want {
                    return Err(format!("{}: {cat:?} has {} tricks, needs {want}", d.name, list.len()));
                }
                for (i, t) in list.iter().enumerate() {
                    if list[..i].iter().any(|o| o.input == t.input) {
                        return Err(format!("{}: {} uses the same keys as another {cat:?}", d.name, t.name));
                    }
                    if !t.secs.is_finite() || t.secs <= 0.0 {
                        return Err(format!("{}: {} needs a positive duration", d.name, t.name));
                    }
                }
            }
            let flips = &d.air_flips;
            if !flips.iter().any(|t| t.anim.body_flip < 0.0) || !flips.iter().any(|t| t.anim.body_flip > 0.0) {
                return Err(format!("{}: needs a front flip and a back flip", d.name));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_data_has_ten_of_each_plus_flips() {
        let lib = MountLibrary::built_in();
        assert_eq!(lib.defs.len(), 6);
        for kind in MountKind::ALL {
            let d = lib.get(kind);
            assert_eq!(d.kind, kind);
            assert_eq!(d.flips.len(), 10, "{}", d.name);
            assert_eq!(d.grabs.len(), 10, "{}", d.name);
            assert_eq!(d.grinds.len(), 10, "{}", d.name);
            assert_eq!(d.air_flips.len(), 2, "{}", d.name);
        }
        lib.check().unwrap();
    }

    #[test]
    fn soap_shoes_do_every_inline_skate_trick() {
        let lib = MountLibrary::built_in();
        let (soap, skates) = (lib.get(MountKind::SoapShoes), lib.get(MountKind::InlineSkates));
        assert_eq!(soap.flips, skates.flips);
        assert_eq!(soap.grabs, skates.grabs);
        assert_eq!(soap.grinds, skates.grinds);
        assert_ne!(soap.stats, skates.stats, "but they ride differently");
    }

    #[test]
    fn longboard_is_a_bigger_slower_flipping_skateboard() {
        let lib = MountLibrary::built_in();
        let (long, board) = (lib.get(MountKind::Longboard), lib.get(MountKind::Skateboard));
        assert!(long.stats.top_speed > board.stats.top_speed);
        assert!(long.stats.turn < board.stats.turn && long.stats.jump < board.stats.jump);
        let avg = |d: &MountDef| d.flips.iter().map(|t| t.secs).sum::<f32>() / 10.0;
        assert!(avg(long) > avg(board), "the long deck flips slower");
    }

    #[test]
    fn keys_pick_tricks_with_fallback() {
        let lib = MountLibrary::built_in();
        let d = lib.get(MountKind::Skateboard);
        let kick = d.find(TrickCat::Flip, TrickInput { dir: Dir::None, shift: false }).unwrap();
        assert_eq!(d.flips[kick].name, "Kickflip");
        let laser = d.find(TrickCat::Flip, TrickInput { dir: Dir::None, shift: true }).unwrap();
        assert_eq!(d.flips[laser].name, "Laser Flip");
        // Shift + a direction with no shifted trick falls back to the plain one.
        let hard = d.find(TrickCat::Flip, TrickInput { dir: Dir::UpRight, shift: true }).unwrap();
        assert_eq!(d.flips[hard].name, "Hardflip");
        let back = d.find(TrickCat::AirFlip, TrickInput { dir: Dir::Down, shift: false }).unwrap();
        assert!(d.air_flips[back].anim.body_flip > 0.0);
        // No air flip with no direction: falls back to... nothing exact, so the first
        // matching default; there is none, so None.
        assert_eq!(d.find(TrickCat::AirFlip, TrickInput::default()), None);
    }

    #[test]
    fn keys_map_to_directions() {
        assert_eq!(Dir::from_keys(true, false, true, false), Dir::UpLeft);
        assert_eq!(Dir::from_keys(false, true, false, true), Dir::DownRight);
        assert_eq!(Dir::from_keys(false, false, false, true), Dir::Right);
        assert_eq!(Dir::from_keys(true, true, false, false), Dir::None);
    }

    #[test]
    fn bad_data_is_rejected_with_a_reason() {
        let mut files: Vec<(&str, String)> = BUILT_IN.iter().map(|(n, t)| (*n, t.to_string())).collect();
        files[0].1 = files[0].1.replace("(name: \"Heelflip\",        input: (dir: Down)", "(name: \"Heelflip\",        input: (dir: None)");
        let err = MountLibrary::parse(&files).unwrap_err();
        assert!(err.contains("same keys"), "{err}");
        files[0].1 = "(oops".to_string();
        assert!(MountLibrary::parse(&files).unwrap_err().starts_with("skateboard.ron"));
    }

    #[test]
    fn a_data_folder_overrides_the_built_in_files() {
        let dir = std::env::temp_dir().join(format!("brawl_mounts_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (n, t) in BUILT_IN {
            std::fs::write(dir.join(n), t.replace("\"Kickflip\"", "\"Super Kickflip\"")).unwrap();
        }
        let (lib, msg) = MountLibrary::load(&dir);
        assert!(msg.contains("loaded from"), "{msg}");
        assert_eq!(lib.get(MountKind::Skateboard).flips[0].name, "Super Kickflip");
        // A broken file: fall back to the built-in data and say why.
        std::fs::write(dir.join("bmx.ron"), "(broken").unwrap();
        let (lib, msg) = MountLibrary::load(&dir);
        assert!(msg.contains("not used"), "{msg}");
        assert_eq!(lib.get(MountKind::Skateboard).flips[0].name, "Kickflip");
        std::fs::remove_dir_all(&dir).ok();
        let (_, msg) = MountLibrary::load(std::path::Path::new("definitely/not/here"));
        assert_eq!(msg, "built-in mount data");
    }
}
