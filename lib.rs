#[path = "BaseEntity.rs"]
mod base_entity;
#[path = "BaseSystem/mod.rs"]
mod base_system;
#[path = "Host/mod.rs"]
mod host;

#[path = "Platform/Desktop.rs"]
mod desktop;
pub fn run_cli(args: &[String]) -> Result<(), String> {
    desktop::run(args)
}

#[path = "Platform/iOS/Bridge.rs"]
mod ios_bridge;
