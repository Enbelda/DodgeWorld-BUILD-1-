//! Original team dodgeball brawler: game library (simulation, networking, Bevy client).

pub mod arena;
pub mod autopilot;
pub mod camera;
pub mod config;
pub mod hud;
pub mod net;
pub mod player;
pub mod render;
pub mod rig;
pub mod session;
pub mod sim;

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use config::LaunchConfig;

/// Build and run the game window.
pub fn run(cfg: LaunchConfig) -> Result<(), String> {
    let (session, status) = session::build_session(&cfg)?;
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
        .insert_resource(session::View { status, ..default() })
        .add_plugins((
            arena::ArenaPlugin,
            camera::CameraPlugin,
            player::PlayerPlugin,
            session::SessionPlugin,
            render::RenderPlugin,
            hud::HudPlugin,
        ))
        .add_systems(Last, session::leave_on_exit)
        .run();
    Ok(())
}
