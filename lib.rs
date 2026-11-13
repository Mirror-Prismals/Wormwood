#[path = "BaseEntity.rs"]
mod base_entity;
#[path = "BaseSystem/mod.rs"]
mod base_system;
#[path = "Host/mod.rs"]
mod host;
#[path = "Physics/mod.rs"]
mod physics;
#[path = "Terrain/mod.rs"]
pub(crate) mod terrain;

#[path = "Platform/Desktop.rs"]
mod desktop;
pub fn run_cli(args: &[String]) -> Result<(), String> {
    desktop::run(args)
}

#[path = "Platform/iOS/Bridge.rs"]
mod ios_bridge;
#[cfg(any(target_os="ios",test))]
#[path="Platform/iOS/Touch.rs"]
mod ios_touch;

#[path = "Water/mod.rs"]
pub(crate) mod water;
#[path = "Vegetation/mod.rs"]
pub(crate) mod vegetation;
#[path = "Audio/mod.rs"]
pub(crate) mod audio;
#[path = "Render/Water/data.rs"]
pub(crate) mod water_render_data;

#[path="Render/SceneAssets.rs"]
pub(crate) mod scene_assets;
#[path="Render/Limits.rs"]
pub(crate) mod scene_limits;

#[path = "Workstation/mod.rs"]
pub(crate) mod workstation;

#[path = "Theatre/mod.rs"]
pub(crate) mod theatre;

#[path = "Phone/mod.rs"]
pub(crate) mod phone;

#[path="Modding/mod.rs"]
pub(crate) mod modding;
