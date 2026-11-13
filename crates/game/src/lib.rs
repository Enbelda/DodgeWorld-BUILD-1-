//! Original team dodgeball brawler: game library (simulation, networking, Bevy client).

pub mod arena;
pub mod autopilot;
pub mod camera;
pub mod config;
pub mod hud;
pub mod mount_vis;
pub mod mounts;
pub mod cover;
pub mod net;
pub mod parkour;
pub mod player;
pub mod render;
pub mod ride;
pub mod rig;
pub mod session;
pub mod sewer_vis;
pub mod sim;
pub mod textures;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use config::LaunchConfig;

/// Build and run the game window.
pub fn run(cfg: LaunchConfig) -> Result<(), String> {
    let (session, status) = session::build_session(&cfg)?;
    let (mounts, mount_msg) = mounts::MountLibrary::load(std::path::Path::new("data/mounts"));
    info!("{mount_msg}");
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: format!("Brawl - {}", cfg.name), ..default() }),
            ..default()
        }))
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        .insert_resource(ClearColor(Color::srgb(0.53, 0.75, 0.92)))
        .init_resource::<GlobalAmbientLight>()
        .insert_resource(session)
        .insert_resource(player::Autopilot(cfg.autopilot))
        .insert_resource(autopilot::ParkourDrill(cfg.drill))
        .insert_resource(autopilot::RideDrill(cfg.ride_drill))
        .insert_resource(session::View { status, ..default() })
        .insert_resource(mounts)
        .add_plugins((
            textures::TexturesPlugin,
            arena::ArenaPlugin,
            camera::CameraPlugin,
            player::PlayerPlugin,
            session::SessionPlugin,
            render::RenderPlugin,
            mount_vis::MountVisPlugin,
            sewer_vis::SewerPlugin,
            hud::HudPlugin,
        ))
        .add_systems(Last, session::leave_on_exit)
        .run();
    Ok(())
}
