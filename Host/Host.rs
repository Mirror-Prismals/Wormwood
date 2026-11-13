use serde_json::{Value, json};
#[cfg(all(test, target_os = "macos"))]
#[path = "MiningBenchmark.rs"]
mod mining_benchmark;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
struct Hook {
    system: String,
    layer: u64,
    input: String,
    output: String,
    calls: BTreeSet<String>,
}

pub(crate) struct Host {
    pub(crate) mods: crate::modding::Manager,
    root: PathBuf,
    output: PathBuf,
    enabled: BTreeMap<String, bool>,
    hooks: BTreeMap<String, Hook>,
    native_hooks: BTreeMap<String,String>,
    roots: Vec<String>,
    startup_steps: Vec<String>,
    startup_order: Vec<String>,
    startup_attempted: bool,
    all_init: BTreeMap<String, Vec<String>>,
    initialized: BTreeSet<String>,
    initialization_failed: BTreeSet<String>,
    stack: Vec<(u64, String)>,
    next_id: u64,
    halt: bool,
    trace: PathBuf,
    trace_writer: RefCell<Option<(PathBuf, super::trace_output::Writer)>>,
    audio_preparation_journal: Option<std::sync::Arc<std::sync::Mutex<PreparationJournal>>>,
    run_id: String,
    performance: super::performance_telemetry::Telemetry,
    pub(crate) water: crate::water::State,
    pub(crate) vegetation: crate::vegetation::State,
    pub(crate) caves: crate::base_system::layer_two::cave_system::State,
    pub(crate) workstation: crate::workstation::Owner,
    pub(crate) workstation_panels: crate::base_system::layer_four::workstation_panel_system::State,
    pub(crate) security_cameras: crate::theatre::cameras::State,
    pub(crate) audio_workstation: crate::base_system::layer_five::audio_workstation_system::State,
    pub(crate) workstation_export: crate::base_system::layer_five::workstation_export_system::State,
    pub(crate) chuck: crate::base_system::layer_one::chuck_connection_system::State,
    pub(crate) ray_audio: crate::base_system::layer_two::ray_traced_audio_system::State,
    pub(crate) environment_audio: crate::base_system::layer_five::environment_audio_system::State,
    pub(crate) glasses: crate::base_system::layer_five::glasses_system::State,
    pub(crate) glasses_render: crate::base_system::layer_four::glasses_render_system::State,
    pub(crate) crafting: crate::base_system::layer_five::crafting_system::State,
    pub(crate) crafting_render: crate::base_system::layer_four::crafting_render_system::State,
    pub(crate) phone: crate::base_system::layer_five::phone_system::State,
    pub(crate) phone_render: crate::base_system::layer_four::phone_render_system::State,
    pub(crate) theatre: crate::base_system::layer_two::theatre_system::State,
    pub(crate) audio_visualizer: crate::base_system::layer_four::audio_ray_visualizer_system::State,
    pub(crate) monastery: crate::base_system::layer_five::monastery_system::State,
    pub(crate) laboratory: crate::base_system::layer_five::laboratory_system::State,
    pub(crate) npcs: crate::base_system::layer_five::npc_system::State,
    pub(crate) deer: crate::base_system::layer_five::deer_system::State,
    pub(crate) silver_spider_crab: crate::base_system::layer_five::silver_spider_crab_system::State,
    pub(crate) birds: crate::base_system::layer_five::bird_system::State,
    pub(crate) entities: crate::base_system::layer_two::entity_system::State,
    pub(crate) audicles: crate::base_system::layer_five::audicle_system::State,
    pub(crate) ik:crate::base_system::layer_two::inverse_kinematics_system::State,
    pub(crate) player_rig:crate::base_system::layer_four::player_rig_system::State,
    pub(crate) physics: crate::physics::State,
    pub(crate) planet: crate::physics::planet::State,
    pub(crate) terrain_tool: crate::physics::terrain_tool::State,
    pub(crate) shovel_soil: crate::physics::shovel::State,
    pub(crate) shovel: crate::base_system::layer_five::shovel_system::State,
    pub(crate) ogc: crate::base_system::layer_one::ogc_connection_system::State,
    pub(crate) graphics: crate::base_system::layer_one::graphics_connection_system::State,
    pub(crate) scene_preparation:Option<crate::base_system::layer_four::appearance_system::Worker>,
    pub(crate) water_collision_preparation:Option<crate::physics::planet::collision_worker::Worker>,
    pub(crate) color_emotion: crate::base_system::layer_four::color_emotion_system::State,
    pub(crate) active_audicle: Option<u64>,
    pub(crate) audio: crate::base_system::layer_one::jack_connection_system::State,
    pub(crate) ios_audio: crate::base_system::layer_one::ios_audio_connection_system::State,
    pub(crate) environment: crate::base_system::layer_five::environment_system::State,
    pub(crate) flight: crate::base_system::layer_five::flight_system::State,
    pub(crate) cue_session: crate::base_system::layer_five::cue_system::State,
    pub(crate) verify_session: bool,
    audio_enabled: bool,
    backend: String,
    frame_limit: u32,
    frame_period_us: u64,
    live: bool,
    close_after_frames: Option<u32>,
    pub(crate) game_workstation_benchmark: Option<Value>,
}

fn read(root: &Path, path: &str) -> Result<Value, String> {
    let bytes = fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}
fn object<'a>(value: &'a Value, label: &str) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}
fn names(value: &Value, label: &str) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{label} must be an array"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{label} needs string entries"))
        })
        .collect()
}
fn identifier(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
fn bounded_data(value: &Value) -> bool {
    let mut pending = vec![(value,0usize)];
    let mut count = 0usize;
    while let Some((value,depth)) = pending.pop() {
        count += 1;
        if depth > 24 || count > 8_000_000 { return false; }
        match value {
            Value::Number(n) => if !n.as_f64().is_some_and(f64::is_finite) { return false; },
            Value::String(s) => if s.len() > 16384 { return false; },
            Value::Array(a) => { if a.len() > 393216 { return false; } pending.extend(a.iter().map(|v|(v,depth+1))); },
            Value::Object(o) => { if o.len() > 128 { return false; } pending.extend(o.values().map(|v|(v,depth+1))); },
            _ => {},
        }
    }
    true
}

fn builtin(name: &str) -> Option<(u64, &'static str, &'static str)> {
    match name {
        "CraftingSystem.Configure" | "CraftingSystem.Close" | "CraftingSystem.Status" => Some((5,"Unit","GeometryData")),
        "CraftingSystem.Input" | "CraftingSystem.Frame" | "CraftingSystem.Grip" => Some((5,"GeometryData","GeometryData")),
        "CraftingRenderSystem.Load" => Some((4,"Unit","Unit")),
        "CraftingRenderSystem.Frame" => Some((4,"GeometryData","GeometryData")),
        "PhysicsSystem.GameCraftTree" => Some((2,"GeometryData","GeometryData")),
        "PhoneSystem.Configure" | "PhoneSystem.Close" => Some((5,"Unit","Unit")),
        "PhoneSystem.Status" | "PhoneSystem.Check" => Some((5,"Unit","GeometryData")),
        "PhoneSystem.Input" | "PhoneSystem.Grip" | "PhoneSystem.Frame" | "PhoneSystem.CheckScene" => Some((5,"GeometryData","GeometryData")),
        "GlassesSystem.Configure" | "GlassesSystem.Close" => Some((5,"Unit","Unit")),
        "GlassesSystem.Status" => Some((5,"Unit","GeometryData")),
        "GlassesSystem.Input" | "GlassesSystem.Grip" => Some((5,"GeometryData","GeometryData")),
        "GlassesRenderSystem.Load" => Some((4,"Unit","Unit")),
        "GlassesRenderSystem.Frame" => Some((4,"ViewRequest","GeometryData")),
        "PhoneRenderSystem.Load" => Some((4,"Unit","Unit")),
        "PhoneRenderSystem.Frame" => Some((4,"ViewRequest","GeometryData")),
        "SecurityCameraSystem.View" => Some((5,"WorkstationData","WorkstationData")),
        "SecurityCameraSystem.Status" => Some((5,"Unit","WorkstationData")),
        "TheatreSystem.Configure" => Some((2,"Unit","Unit")),
        "TheatreSystem.Status" => Some((2,"Unit","GeometryData")),
        "TheatreSystem.ChairInput" | "TheatreSystem.ResolveBody" | "TheatreSystem.Trace" | "TheatreSystem.AcousticGeometry" => Some((2,"GeometryData","GeometryData")),
        "TheatreSystem.Mesh" => Some((2,"ViewRequest","GeometryData")),
        "AudioRayVisualizerSystem.Frame" => Some((4,"ViewRequest","GeometryData")),
        "AudioRayVisualizerSystem.Clear" => Some((4,"Unit","Unit")),
        "WorkstationPanelSystem.Load" => Some((4,"Unit","EntitySpec")),
        "WorkstationPanelSystem.View" => Some((4,"WorkstationData","Unit")),
        "WorkstationPanelSystem.Suspend" => Some((4,"Unit","Unit")),
        "WorkstationPanelSystem.Release" => Some((4,"Unit","Unit")),
        "AudioWorkstationSystem.Begin" | "AudioWorkstationSystem.Status" => Some((5,"Unit","WorkstationData")),
        "GraphicsConnectionSystem.ParkWorkstation" | "GraphicsConnectionSystem.FocusWorkstation" => Some((1,"EntityRef","Unit")),
        "GraphicsConnectionSystem.PrewarmWorkstation" => Some((1,"Unit","Unit")),
        "AudioWorkstationSystem.PrimeWorld" => Some((5,"Unit","Unit")),
        "AudioWorkstationSystem.Background" => Some((5,"Unit","Unit")),
        "AudioWorkstationSystem.Prewarm" => Some((5,"Unit","Unit")),
        "AudioWorkstationSystem.PreparePlugins" => Some((5,"Unit","Unit")),
        "AudioWorkstationSystem.Resume" => Some((5,"WorkstationEntry","WorkstationData")),
        "AudioWorkstationSystem.Suspend" => Some((5,"Unit","Unit")),
        "AudioWorkstationSystem.RunEmbedded" => Some((5,"WorkstationData","WorkstationData")),
        "GraphicsConnectionSystem.Suspend" | "GraphicsConnectionSystem.Resume" => Some((1,"EntityRef","Unit")),
        "AudioWorkstationSystem.Step" | "AudioWorkstationSystem.SetPanel" | "AudioWorkstationSystem.DrumRack" | "AudioWorkstationSystem.OpenPianoRoll" | "AudioWorkstationSystem.ComputerKeyboard" | "AudioWorkstationSystem.Run" => Some((5,"WorkstationData","WorkstationData")),
        "AudioWorkstationSystem.Close" => Some((5,"Unit","Unit")),
        "PluginConnectionSystem.Status" => Some((1,"Unit","WorkstationData")),
        "PluginConnectionSystem.Command" | "PluginConnectionSystem.Menu" => Some((1,"WorkstationData","WorkstationData")),
        "WorkstationDataSystem.Snapshot" | "WorkstationDataSystem.CaptureStatus" => Some((2,"Unit","WorkstationData")),
        "WorkstationDataSystem.Undo" | "WorkstationDataSystem.Redo" | "WorkstationDataSystem.UndoStatus" | "WorkstationDataSystem.BeginUndoTransaction" | "WorkstationDataSystem.CommitUndoTransaction" | "WorkstationDataSystem.CancelUndoTransaction" | "WorkstationDataSystem.ClearUndo" |
            "WorkstationDataSystem.TrackFreezeBegin" | "WorkstationDataSystem.TrackFreezeStep" | "WorkstationDataSystem.TrackFreezeAccept" | "WorkstationDataSystem.TrackFreezeCancel" | "WorkstationDataSystem.TrackFreezeFlatten" | "WorkstationDataSystem.TrackFreezeStatus" => Some((2,"WorkstationData","WorkstationData")),
        "WorkstationDataSystem.MixPreview" | "WorkstationDataSystem.StageLighting" => Some((2,"WorkstationData","WorkstationData")),
        "WorkstationDataSystem.PumpWarp" | "WorkstationDataSystem.ExportStatus" => Some((2,"Unit","WorkstationData")),
        "WorkstationDataSystem.ExportBegin" | "WorkstationDataSystem.ExportStep" | "WorkstationDataSystem.ExportCancel" => Some((2,"WorkstationData","WorkstationData")),
        "WorkstationExportSystem.Open" | "WorkstationExportSystem.Step" | "WorkstationExportSystem.Status" | "WorkstationExportSystem.Cancel" | "WorkstationExportSystem.Close" => Some((5,"Unit","WorkstationData")),
        "WorkstationExportSystem.Begin" | "WorkstationExportSystem.Pump" => Some((5,"WorkstationData","WorkstationData")),
        "WorkstationDataSystem.BeginRetrospectiveAudio" | "WorkstationDataSystem.AppendRetrospectiveAudio" | "WorkstationDataSystem.CommitRetrospectiveAudio" | "WorkstationDataSystem.AbortRetrospectiveAudio" | "WorkstationDataSystem.InsertRetrospectiveMidi" | "WorkstationDataSystem.UndoRetrospective" | "WorkstationDataSystem.RedoRetrospective" | "WorkstationDataSystem.DrumRack" | "WorkstationDataSystem.PutBus" | "WorkstationDataSystem.SetBus" | "WorkstationDataSystem.RemoveBus" | "WorkstationDataSystem.PutTrack" | "WorkstationDataSystem.RemoveTrack" | "WorkstationDataSystem.PutClip" | "WorkstationDataSystem.CreateRangeClips" |
        "WorkstationDataSystem.RemoveClip" | "WorkstationDataSystem.TrimAudioClip" | "WorkstationDataSystem.SetClipGain" | "WorkstationDataSystem.SetClipRouting" | "WorkstationDataSystem.SplitClip" | "WorkstationDataSystem.SetClipFades" | "WorkstationDataSystem.MoveClip" | "WorkstationDataSystem.RebaseTimeline" | "WorkstationDataSystem.SetMusicalTime" |
        "WorkstationDataSystem.SetMidiTempo" | "WorkstationDataSystem.SnapMidi" |
        "WorkstationDataSystem.SetTrackMix" | "WorkstationDataSystem.SetCcTarget" |
        "WorkstationDataSystem.ReorderTracks" | "WorkstationDataSystem.PutSource" | "WorkstationDataSystem.ImportAudio" |
        "WorkstationDataSystem.SnapClip" | "WorkstationDataSystem.RenderRange" | "WorkstationDataSystem.CaptureInput" | "WorkstationDataSystem.CycleMidiTake" | "WorkstationDataSystem.AudioPlaylist" |
        "WorkstationDataSystem.SetMusicalTime" | "WorkstationDataSystem.RebaseTimeline" | "WorkstationDataSystem.SetClipWarp" | "WorkstationDataSystem.SetWarpMap" | "WorkstationDataSystem.AddSpectralMask" | "WorkstationDataSystem.UndoSpectralMask" | "WorkstationDataSystem.ClearSpectralMasks" | "WorkstationDataSystem.ClipEditorPreview" |
        "WorkstationDataSystem.PianoRoll" | "WorkstationDataSystem.BeginMasterAnalysis" | "WorkstationDataSystem.AnalyzeMaster" |
        "WorkstationDataSystem.LocatePanel" | "WorkstationDataSystem.ProjectPanel" => Some((2,"WorkstationData","WorkstationData")),
        "CaveSystem.Configure" | "CaveSystem.Snapshot" => Some((2,"Unit","CaveData")),
        "CaveSystem.Sample" | "CaveSystem.Support" | "CaveSystem.Mesh" | "CaveSystem.ResolveBody" => Some((2,"CaveData","CaveData")),
        "RayTracedAudioSystem.AdvanceTerrain" => Some((2,"AudioData","AudioData")),
        "EnvironmentAudioSystem.PreviewAudio" | "EnvironmentAudioSystem.RefreshMix" | "EnvironmentAudioSystem.Begin" | "EnvironmentAudioSystem.Frame" | "EnvironmentAudioSystem.Close" | "EnvironmentAudioSystem.Status" | "EnvironmentAudioSystem.Transport" | "EnvironmentAudioSystem.Pump" | "EnvironmentAudioSystem.Retrospective" | "EnvironmentAudioSystem.DrainCapture" | "EnvironmentAudioSystem.MidiInput" | "EnvironmentAudioSystem.MidiKeyboard" => Some((5,"AudioData","AudioData")),
        "TerrainGeometrySystem.Capture" | "TerrainGeometrySystem.Trace" | "TerrainGeometrySystem.MaterialIntervals" => Some((2,"GeometryData","GeometryData")),
        "ChuckConnectionSystem.AttachOutput" => Some((1,"OutputAttachment","Unit")),
        "JackConnectionSystem.Attach" | "IOSAudioConnectionSystem.Attach" => Some((1,"EntityRef","Unit")),
        "JackConnectionSystem.Info" | "IOSAudioConnectionSystem.Info" => Some((1,"EntityRef","AudioData")),
        "TideSystem.Configure" => Some((2,"WaterData","WaterData")),
        "TideSystem.Step" => Some((2,"WaterData","WaterData")),
        "TideSystem.Sample" => Some((2,"WaterData","WaterData")),
        "TideSystem.Close" => Some((2,"WaterData","WaterData")),
        "OceanWaveSystem.Configure" => Some((2,"WaterData","WaterData")),
        "OceanWaveSystem.Step" => Some((2,"WaterData","WaterData")),
        "OceanWaveSystem.Sample" => Some((2,"WaterData","WaterData")),
        "OceanWaveSystem.Snapshot" => Some((2,"WaterData","WaterData")),
        "OceanWaveSystem.Close" => Some((2,"WaterData","WaterData")),
        "WaterFlowSystem.Configure" => Some((2,"WaterData","WaterData")),
        "WaterFlowSystem.Step" => Some((2,"WaterData","WaterData")),
        "WaterFlowSystem.Sample" => Some((2,"WaterData","WaterData")),
        "WaterFlowSystem.Close" => Some((2,"WaterData","WaterData")),
        "WaterSlideSystem.Configure" => Some((2,"WaterData","WaterData")),
        "WaterSlideSystem.Edit" => Some((2,"WaterData","WaterData")),
        "WaterSlideSystem.Course" => Some((2,"WaterData","WaterData")),
        "WaterSlideSystem.CollisionMesh" => Some((2,"WaterData","WaterData")),
        "WaterSlideSystem.Snapshot" => Some((2,"WaterData","WaterData")),
        "WaterSlideSystem.Close" => Some((2,"WaterData","WaterData")),
        "FloodGateSystem.Configure" => Some((2,"WaterData","WaterData")),
        "FloodGateSystem.Step" => Some((2,"WaterData","WaterData")),
        "FloodGateSystem.Close" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Configure" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Step" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Gates" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Sample" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Disturb" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Snapshot" => Some((2,"WaterData","WaterData")),
        "SoftBodyWaterSystem.Close" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.MaterialPreview" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.MaterialDomain" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Configure" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Update" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Raycast" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.AudioSources" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Sample" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.SurfaceMesh" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Drainage" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Revision" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Snapshot" => Some((2,"WaterData","WaterData")),
        "TerrainSurfaceHydrologySystem.Close" => Some((2,"WaterData","WaterData")),
        "WaterRenderSystem.Prepare" => Some((4,"WaterData","WaterData")),
        "WaterRenderSystem.Update" => Some((4,"WaterData","WaterData")),
        "WaterRenderSystem.Frame" => Some((4,"WaterData","WaterData")),
        "WaterRenderSystem.Close" => Some((4,"WaterData","WaterData")),
        "WaterSystem.Configure" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Step" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Frame" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Sample" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Disturb" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Edit" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Pause" => Some((5,"WaterData","WaterData")),
        "WaterSystem.Close" => Some((5,"WaterData","WaterData")),
        "VegetationSystem.QueryPerches" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.QueryPlants" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.MaterialTree" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.QueryWood" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.QueryGeometry" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.AdvanceGrowth" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.ApplyDamage" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.Snapshot" => Some((2,"VegetationData","VegetationData")),
        "VegetationSystem.Restore" => Some((2,"VegetationData","VegetationData")),
        "ChuckConnectionSystem.Prepare" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.Compile" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.Remove" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.Poll" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.RefreshMix" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.SetMicrophoneRoutes" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.SetRoutes" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.Telemetry" => Some((1,"AudioData","AudioData")),
        "ChuckConnectionSystem.Release" => Some((1,"AudioData","AudioData")),
        "RayTracedAudioSystem.Advance" => Some((2,"AudioData","AudioData")),
        "RayTracedAudioSystem.Snapshot" => Some((2,"AudioData","AudioData")),
        "RayTracedAudioSystem.Invalidate" => Some((2,"AudioData","AudioData")),
        "RayTracedAudioSystem.AdvanceMicrophone" => Some((2,"AudioData","AudioData")),
        "SoundPhysicsSystem.Apply" => Some((2,"AudioData","AudioData")),
        "SoundPhysicsSystem.ApplyMicrophone" => Some((2,"AudioData","AudioData")),
        "MonasterySystem.Input" | "MonasterySystem.Snapshot" | "MonasterySystem.Close" => Some((5,"GeometryData","GeometryData")),
        "MonasteryRenderSystem.Frame" => Some((4,"ViewRequest","GeometryData")),
        "LaboratorySystem.Step" | "LaboratorySystem.Snapshot" | "LaboratorySystem.Close" => Some((5,"GeometryData","GeometryData")),
        "NpcSystem.Step" | "NpcSystem.Snapshot" | "NpcSystem.Close" => Some((5,"GeometryData","GeometryData")),
        "DeerSystem.Step" | "DeerSystem.Snapshot" | "DeerSystem.Close" => Some((5,"GeometryData","GeometryData")),
        "SilverSpiderCrabSystem.Step" | "SilverSpiderCrabSystem.Close" => Some((5,"GeometryData","GeometryData")),
        "BirdSystem.Step" => Some((5,"AudioData","AudioData")),
        "BirdSystem.Snapshot" => Some((5,"AudioData","AudioData")),
        "BirdSystem.Close" => Some((5,"AudioData","AudioData")),
        "RegistrySystem.State" => Some((2,"Unit","RegistryState")),
        "RegistrySystem.SetEnabled" => Some((2,"SystemToggle","RegistryState")),
        "AudicleSystem.Fail" => Some((5,"AudicleFailure","Unit")),
        "AudicleSystem.Resume" => Some((5,"AudicleRef","Unit")),
        "PlanetNavigationSystem.Abort" | "FlightSystem.Abort" | "ShovelSystem.Abort" => Some((5,"FailureReason","Unit")),
        "PlayerModelSystem.Load"=>Some((2,"Unit","PlayerModelData")),
        "InverseKinematicsSystem.Configure"=>Some((2,"PlayerModelData","Unit")),
        "InverseKinematicsSystem.SolveChains"=>Some((2,"GeometryData","GeometryData")),
        "InverseKinematicsSystem.Solve"=>Some((2,"RigInput","RigPose")),
        "InverseKinematicsSystem.Active"=>Some((2,"Unit","Boolean")),
        "InverseKinematicsSystem.Release"=>Some((2,"Unit","Unit")),
        "PlayerRigSystem.Prepare"=>Some((4,"EntityRef","Unit")),
        "PlayerRigSystem.Plan"=>Some((4,"RigInput","RigPose")),
        "PlayerRigSystem.Advance"=>Some((4,"RigInput","RigView")),
        "PlayerRigSystem.Release"=>Some((4,"EntityRef","Unit")),
        "GraphicsConnectionSystem.Texture"=>Some((1,"TextureRequest","Unit")),
        "GraphicsConnectionSystem.ReleaseTexture"=>Some((1,"TextureOwner","Unit")),
        "JackConnectionSystem.RouteChanged" => Some((1, "EntityRef", "Boolean")),
        "JackConnectionSystem.Complete" => Some((1, "EntityRef", "Boolean")),
        "GraphicsConnectionSystem.InputMode" => Some((1,"InputModeRequest","Unit")),
        "PlanetNavigationSystem.Configure" | "PlanetNavigationSystem.Pause" => Some((5,"Unit","Unit")),
        "GraphicsConnectionSystem.Pump" => Some((1, "EntityRef", "WindowFrame")),
        "AppearanceSystem.Pump" => Some((4, "EntityRef", "WindowFrame")),
        "ApplicationSystem.Run" => Some((5, "ProcedureRef", "SessionReport")),
        "CueSystem.Begin" => Some((5, "ProcedureRef", "Unit")),
        "CueSystem.Advance" => Some((5, "Unit", "CueStatus")),
        "CueSystem.Cancel" => Some((5, "Unit", "Unit")),
        "CueSystem.Close" => Some((5, "Unit", "Unit")),
        "CueSystem.Status" => Some((5, "Unit", "CueStatus")),
        "PlanetNavigationSystem.Step" => Some((5, "EnvironmentInput", "FlightState")),
        "PlanetNavigationSystem.Stop" => Some((5, "Unit", "Unit")),
        "FlightSystem.Step" => Some((5, "FlightInput", "FlightState")),
        "FlightSystem.Stop" => Some((5, "Unit", "Unit")),
        "ColorEmotionSystem.Advance" => Some((4,"ColorEmotionInput","ColorEmotionView")),
        "ColorEmotionSystem.Active" => Some((4,"Unit","Boolean")),
        "ColorEmotionSystem.Release" => Some((4,"Unit","Unit")),
        "CloudSystem.Frame" => Some((4,"ViewRequest","GeometryData")),
        "AppearanceSystem.View" => Some((4, "ViewRequest", "Unit")),
        "GraphicsConnectionSystem.SetView" => Some((1, "TriangleBatch", "Unit")),
        "OGCConnectionSystem.Respond" => Some((1,"ContactResponseRequest","ContactResponseResult")),
        "OGCConnectionSystem.Synchronize" => Some((1,"ContactSync","Unit")),
        "OGCConnectionSystem.Ownership" => Some((1,"Unit","ContactOwnership")),
        "OGCConnectionSystem.Configure" => Some((1, "PhysicsScene", "Unit")),
        "OGCConnectionSystem.Project" => Some((1, "ContactRequest", "ContactResult")),
        "WorkshopSystem.Run" => Some((5, "SimulationRequest", "SimulationReport")),
        "EntitySystem.Update" => Some((2, "EntityUpdate", "Unit")),
        "GraphicsConnectionSystem.Caption" => Some((1, "CaptionRequest", "Unit")),
        "GraphicsConnectionSystem.CursorLabel" => Some((1, "CursorLabelRequest", "Unit")),
        "GraphicsConnectionSystem.PluginSurface" => Some((1,"PluginSurfaceRequest","Unit")),
        "GraphicsConnectionSystem.CcParameterPicker" => Some((1,"CcParameterPickerRequest","Unit")),
        "GraphicsConnectionSystem.SelectionRange" => Some((1,"SelectionRangeRequest","Unit")),
        "WorkshopSystem.Benchmark" => Some((5, "SimulationRequest", "SimulationReport")),
        "OGCConnectionSystem.Release" => Some((1, "Unit", "Unit")),
        "ShovelSystem.Step" => Some((5,"ShovelInput","ShovelView")),
        "ShovelSystem.ConstrainMovement" => Some((5,"ToolBodyMotion","ToolBodyConstraint")),
        "ShovelSystem.Active" => Some((5,"Unit","Boolean")),
        "ShovelSystem.Close" => Some((5,"Unit","Unit")),
        "ShovelSoilSystem.Configure" => Some((2,"ShovelConfiguration","ShovelState")),
        "ShovelSoilSystem.Step" => Some((2,"ShovelStep","ShovelState")),
        "ShovelSoilSystem.Report" => Some((2,"Unit","ShovelState")),
        "ShovelSoilSystem.Mesh" => Some((2,"Unit","EntitySpec")),
        "ShovelSoilSystem.Release" => Some((2,"Unit","Unit")),
        "PlanetSurfaceSystem.SuspendMaterial" => Some((2,"Unit","Unit")),
        "TerrainToolSystem.Aim" => Some((2,"ToolRay","ToolAim")),
        "TerrainToolSystem.SurfaceHeight" => Some((2,"SurfaceQuery","MaterialHeight")),
        "TerrainToolSystem.Cut" => Some((2,"ToolCut","ToolCutResult")),
        "TerrainToolSystem.Advance" => Some((2,"ToolMotion","ToolContact")),
        "TerrainToolSystem.Prepare" => Some((2,"ToolMotion","ToolContact")),
        "TerrainToolSystem.Frame" => Some((2,"Unit","RemovedSoilFrame")),
        "TerrainToolSystem.Release" => Some((2,"Unit","Unit")),
        "PlanetSurfaceSystem.Reset" => Some((2,"Unit","Unit")),
        "PlanetSurfaceSystem.ReleaseMaterials" => Some((2,"Unit","Unit")),
        "PlanetSurfaceSystem.RefineMaterial" => Some((2,"RefineMaterial","Unit")),
        "PlanetSurfaceSystem.MineCell" => Some((2,"MineCell","ToolCutResult")),
        "PlanetSurfaceSystem.BladeCut" => Some((2,"BladeCut","Unit")),
        "PlanetSurfaceSystem.CollisionSnapshot" => Some((2,"WaterData","WaterData")),
        "PlanetSurfaceSystem.Frame" => Some((2, "Unit", "PlanetFrame")),
        "PlanetSurfaceSystem.Height" => Some((2, "SurfaceQuery", "SurfaceHeight")),
        "PlanetSurfaceSystem.Excavate" => Some((2, "ExcavationRequest", "ExcavationResult")),
        "PlanetSurfaceSystem.Advance" => Some((2, "Unit", "PlanetFrame")),
        "PlanetSurfaceSystem.Save" => Some((2, "Unit", "Unit")),
        "PhysicsSystem.GameStep" => Some((2,"GameMaterialInput","GameMaterialData")),
        "PhysicsSystem.GameFrame" => Some((2,"Unit","GameMaterialData")),
        "PhysicsSystem.GameResolveBody" => Some((2,"WaterData","WaterData")),
        "PhysicsSystem.GameWaterSample" => Some((2,"WaterData","WaterData")),
        "PhysicsSystem.GameWater" => Some((2,"Unit","WaterData")),
        "PhysicsSystem.GameCollision" => Some((2,"Unit","GameMaterialData")),
        "PhysicsSystem.GameHeight" => Some((2,"SurfaceQuery","MaterialHeight")),
        "PhysicsSystem.GamePause" | "PhysicsSystem.GameClose" => Some((2,"Unit","Unit")),
        "PhysicsSystem.Configure" => Some((2, "SimulationRequest", "PhysicsReport")),
        "PhysicsSystem.Advance" => Some((2, "Unit", "PhysicsReport")),
        "PhysicsSystem.Release" => Some((2, "Unit", "Unit")),
        "PhysicsSystem.Report" => Some((2, "Unit", "PhysicsReport")),
        "PhysicsSystem.Mesh" => Some((2, "Unit", "EntitySpec")),
        "NativeInventorySystem.Probe" => Some((1, "Unit", "NativeInventory")),
        "DiagnosticsSystem.ValidateRuntime" => Some((2, "Unit", "RuntimeValidation")),
        "DiagnosticsSystem.Report" => Some((2, "Unit", "NativeInventory")),
        "DiagnosticsSystem.CheckAudio" => Some((2, "Unit", "JackDiagnostic")),
        "JackConnectionSystem.CheckConnection" => Some((1, "Unit", "JackDiagnostic")),
        "JackConnectionSystem.Prepare" => Some((1, "EntityData", "Unit")),
        "JackConnectionSystem.Start" => Some((1, "EntityRef", "Unit")),
        "JackConnectionSystem.Stop" => Some((1, "EntityRef", "AudioReport")),
        "JackConnectionSystem.Release" => Some((1, "EntityRef", "Unit")),
        "IOSAudioConnectionSystem.Complete" => Some((1,"EntityRef","Boolean")),
        "IOSAudioConnectionSystem.RouteChanged" => Some((1,"EntityRef","Boolean")),
        "IOSAudioConnectionSystem.Prepare" => Some((1, "EntityData", "Unit")),
        "IOSAudioConnectionSystem.Start" => Some((1, "EntityRef", "Unit")),
        "IOSAudioConnectionSystem.Stop" => Some((1, "EntityRef", "IOSAudioReport")),
        "IOSAudioConnectionSystem.Release" => Some((1, "EntityRef", "Unit")),
        "EntitySystem.Create" => Some((2, "EntitySpec", "EntityRef")),
        "EntitySystem.Get" => Some((2, "EntityRef", "EntityData")),
        "EntitySystem.Remove" => Some((2, "EntityRef", "Unit")),
        "AudicleSystem.Create" => Some((5, "Unit", "AudicleRef")),
        "AudicleSystem.Ready" => Some((5, "AudicleRef", "Unit")),
        "AudicleSystem.Go" => Some((5, "AudicleRef", "Unit")),
        "AudicleSystem.Finish" => Some((5, "AudicleRef", "Unit")),
        "AudicleSystem.Cancel" => Some((5, "AudicleRef", "Unit")),
        "AppearanceSystem.Prepare" => Some((4, "EntityRef", "Unit")),
        "AppearanceSystem.Present" => Some((4, "EntityRef", "Presentation")),
        "AppearanceSystem.Release" => Some((4, "EntityRef", "Unit")),
        "GraphicsConnectionSystem.Prepare" => Some((1, "EntityData", "Unit")),
        "GraphicsConnectionSystem.Present" => Some((1, "EntityRef", "Presentation")),
        "GraphicsConnectionSystem.Release" => Some((1, "EntityRef", "Unit")),
        "CueSystem.Run" => Some((5, "ProcedureRef", "CueResult")),
        "CueProcedureSystem.Load" => Some((2, "ProcedureRef", "CueDefinition")),
        _ => None,
    }
}
fn check_payload(kind: &str, value: &Value) -> bool {
    match kind {
        "Unit" => value.is_null(),
        "WorkstationEntry" => value.is_null() || value.as_object().is_some_and(|o|o.keys().all(|k|k=="eyewear" || k=="computer_screen") && o.get("computer_screen").is_none_or(Value::is_boolean) && (value["eyewear"].is_null() || ["ar","vr"].iter().any(|k|value["eyewear"]==*k))),
        "Boolean" => value.is_boolean(),
        "ShovelInput" => crate::base_system::layer_five::shovel_system::valid(value) && check_payload("EntityRef",value),
        "ShovelView" => crate::base_system::layer_five::shovel_system::valid_view(value),
        "ShovelConfiguration" => crate::physics::shovel::valid_configuration(value),
        "ShovelStep" => crate::physics::shovel::valid_step(value),
        "ShovelState" => value["active"].is_boolean() && value["halted"].is_boolean() && value["stage"].is_string(),
        "PhysicsScene" => crate::base_system::layer_one::ogc_connection_system::valid_scene(value),
        "WaterData" => crate::water::valid_data(value),
        "VegetationData" | "AudioData" | "GeometryData" | "CaveData" | "WorkstationData" => bounded_data(value),
        "OutputAttachment" => check_payload("EntityRef",value) && matches!(value["backend"].as_str(),Some("jack" | "ios")),
        "SystemToggle" => value.as_object().is_some_and(|o| o.len() == 2)
            && value["system"].as_str().is_some_and(identifier) && value["enabled"].is_boolean(),
        "RegistryState" => value["systems"].as_object().is_some_and(|o| o.values().all(Value::is_boolean))
            && value["initialized"].is_array() && value["initialization_failed"].is_array(),
        "FailureReason" => value["reason"].as_str().is_some_and(|s| !s.trim().is_empty() && s.len() <= 16384),
        "AudicleFailure" => check_payload("AudicleRef", value) && check_payload("FailureReason", value),
        "ContactOwnership" => value["configured"].is_boolean()
            && value["owned_by_caller"].is_boolean()
            && (value["owner"].is_null() || value["owner"].is_string()),
        "ContactSync" => crate::base_system::layer_one::ogc_connection_system::valid_sync(value),
        "ContactRequest" => {
            crate::base_system::layer_one::ogc_connection_system::valid_request(value)
        }
        "ContactResult" => value["positions"].is_array() && value["contact_overflows"].is_u64(),
        "EntityUpdate" => {
            value["kind"] == "visual"
                && check_payload("EntityRef", value)
                && crate::base_entity::Entity::from_spec(1, value).is_ok()
        }
        "PluginSurfaceRequest"=>check_payload("EntityRef",value)&&crate::workstation::plugin_surface::valid(&value["surface"]),
        "CcParameterPickerRequest"=>{
            let numbers=|v:&Value,count:usize|v.as_array().is_some_and(|a|a.len()==count&&a.iter().all(|n|n.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=131072.)));
            let picker=&value["picker"];
            check_payload("EntityRef",value)&&(picker.is_null()||(numbers(&picker["rect"],4)&&numbers(&picker["viewport"],2)&&picker["rows"].as_array().is_some_and(|rows|rows.len()<=4096&&rows.iter().all(|row|numbers(&row["rect"],4)&&numbers(&row["text_origin"],2)&&row["label"].as_str().is_some_and(|text|text.len()<=128)))))
        }
        "SelectionRangeRequest"=>{
            let rect=|v:&Value|v.as_array().is_some_and(|a|a.len()==4&&a.iter().all(|n|n.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=32768.))&&a[2].as_f64().unwrap()>=0.&&a[3].as_f64().unwrap()>=0.);
            let r=&value["range"];
            let point=|v:&Value|v.as_array().is_some_and(|a|a.len()==2&&a.iter().all(|n|n.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=32768.)));
            let selection=r.get("rect").is_none()||(rect(&r["rect"])&&r["holes"].as_array().is_some_and(|a|a.len()<=129&&a.iter().all(rect)));
            let fades=r.get("fades").is_none()||r["fades"].as_array().is_some_and(|a|a.len()<=128&&a.iter().all(|f|{let cc=f["kind"]=="cc";rect(&f["clip"])&&f["points"].as_array().is_some_and(|p|p.len()<=if cc{2050}else{35}&&p.iter().all(point))&&f["handles"].as_array().is_some_and(|p|p.len()<=if cc{2048}else{2}&&p.iter().all(point))}));
            check_payload("EntityRef",value)&&(r.is_null()||(r.is_object()&&selection&&fades&&(r.get("rect").is_some()||r.get("fades").is_some())))
        }
        "CursorLabelRequest" => {
            let l=&value["label"];
            let preview=l.get("preview").is_none()||l["preview"].as_str().is_some_and(|s|matches!(s,"audio"|"midi"|"cc"|"comp"|"matrix"|"mix"|"channel"|"eq"|"tape"|"plugin"));
            let wave=l.get("wave").is_none()||l["wave"].is_null()||l["wave"].as_array().is_some_and(|a|a.len()==128&&a.iter().all(|p|p.as_array().is_some_and(|p|p.len()==2&&p.iter().all(|n|n.as_f64().is_some_and(|n|n.is_finite()&&(-1. ..=1.).contains(&n))))));
            let eq=l["preview"]!="eq"||check_payload("PluginSurfaceRequest",&serde_json::json!({"entity":value["entity"],"surface":{"groups":[{"clip":[0.,0.,300.,220.],"graphs":[l["eq"].clone()],"labels":[]}]}}));
            check_payload("EntityRef",value) && (l.is_null() || (preview&&wave&&eq&&
                l["text"].as_str().is_some_and(|s|!s.is_empty()&&s.len()<=64&&!s.chars().any(char::is_control))
                && l["position"].as_array().is_some_and(|p|p.len()==2&&p.iter().all(|v|v.as_f64().is_some_and(|n|n.is_finite()&&n.abs()<=32768.)))))
        }
        "CaptionRequest" => {
            check_payload("EntityRef", value)
                && value["text"]
                    .as_str()
                    .is_some_and(|s| s.len() <= 512 && !s.contains('\0'))
        }
        "GameMaterialInput" => crate::physics::matter::game::valid(value),
        "GameMaterialData" => value.is_object(),
        "SimulationRequest" => crate::physics::valid_request(value),
        "SimulationReport" => {
            value["mode"] == "simulation_workshop" && value["physics"]["halted"].is_boolean()
        }
        "PlanetFrame" => {
            value["edits"].is_array() && value["debris"].is_array() && value["active"].is_boolean()
        }
        "MaterialHeight" => value["height"].is_null() || value["height"].as_f64().is_some_and(f64::is_finite),
        "RefineMaterial" => crate::terrain::tool_cut::valid_edits(&json!([value["edit"]])),
        "ToolRay" => crate::physics::terrain_tool::valid_ray(value),
        "ToolAim" => value["hit"].is_boolean() && (value["hit"]==false || crate::physics::terrain_tool::valid_ray(&json!({"position":value["position"],"forward":value["normal"]}))),
        "ToolCut" => crate::physics::terrain_tool::valid_cut(value),
        "ToolCutResult" => value["changed"].is_boolean(),
        "ToolMotion" => crate::physics::terrain_tool::valid_motion(value),
        "ToolBodyMotion" => crate::base_system::layer_five::shovel_system::valid_body_motion(value),
        "ToolBodyConstraint" => crate::base_system::layer_five::shovel_system::valid_body_constraint(value),
        "ToolContact" => crate::physics::terrain_tool::valid_pose(&value["pose"]) && value["active"].is_boolean() && value["blocked"].is_boolean(),
        "RemovedSoilFrame" => value["debris"].as_array().is_some_and(|a|a.len()<=12288&&a.len()%3==0&&a.iter().all(|v|v.as_array().is_some_and(|a|a.len()==6&&a.iter().all(|n|n.as_f64().is_some_and(f64::is_finite))))) && value["active"].is_boolean(),
        "MineCell" => ["point","direction","eye"].iter().all(|k| crate::physics::planet::valid(&json!({"position":value[*k],"forward":value[*k]}))) && value["fill"].is_boolean() && value["slot"].as_u64().is_some_and(|s|s<2),
        "BladeCut" => crate::terrain::tool_cut::valid_edits(&json!([value["edit"]])) && value["volume"].as_f64().is_some_and(|v|v.is_finite()&&v>0.&&v<1.),
        "SurfaceQuery" => value["position"].as_array().is_some_and(|v| {
            v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
        }) && (value["camera_eye"].is_null() || value["camera_eye"].as_array().is_some_and(|v| {
            v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
        })),
        "SurfaceHeight" => {
            value["height"].as_f64().is_some_and(f64::is_finite) && value["active"].is_boolean()
        }
        "ExcavationRequest" => crate::physics::planet::valid(value),
        "ExcavationResult" => value["changed"].is_boolean(),
        "ContactResponseRequest" => crate::base_system::layer_one::ogc_connection_system::valid_response_request(value),
        "ContactResponseResult" => value["responses"].as_array().is_some_and(|r| r.len()<=8192 && r.iter().all(|v| v.as_array().is_some_and(|v|v.len()==13 && v.iter().all(|x|x.as_f64().is_some_and(f64::is_finite))))) && value["contacts"].is_u64(),
        "PhysicsReport" => value["frames"].is_u64() && value["halted"].is_boolean(),
        "PlayerModelData"=>crate::base_system::layer_two::player_model_system::validate(value),
        "RigInput"=>crate::base_system::layer_two::inverse_kinematics_system::valid(value),
        "RigPose"=>crate::base_system::layer_two::inverse_kinematics_system::valid_pose(value),
        "RigView"=>check_payload("EntityRef",value)&&crate::base_system::layer_two::player_model_system::math::vector(&value["view_offset"]).is_ok()&&crate::base_system::layer_two::inverse_kinematics_system::valid_tool_feedback(&value["tool_grip"])&&value["tool_reach"].is_object()&&crate::base_system::layer_two::inverse_kinematics_system::valid_tool_reach(&value["tool_reach"])&&crate::base_system::layer_two::player_model_system::math::vector(&value["posture_offset"]).is_ok(),
        "TextureOwner"=>check_payload("EntityRef",value)&&value["owner"].as_u64().is_some_and(|n|n>0),
        "TextureRequest"=>check_payload("TextureOwner",value)&&value["path"]=="Procedures/Assets/Player/prismal_player.rgba",
        "InputModeRequest" => check_payload("EntityRef",value)&&value["pointer"].is_boolean()
            && (value["pause_menu"].is_null() || ["cheats","flying","noclip"].iter().all(|k|value["pause_menu"][*k].is_boolean())
                && (value["pause_menu"]["time_override_minutes"].is_null() || value["pause_menu"]["time_override_minutes"].as_i64().is_some_and(|m|(-1..1440).contains(&m)))),
        "EnvironmentInput" => crate::base_system::layer_five::environment_system::valid(value),
        "FlightInput" => crate::base_system::layer_five::flight_system::valid(value),
        "FlightState" => {
            value["moving"].is_boolean()
                && value["position"].as_array().is_some_and(|v| {
                    v.len() == 3 && v.iter().all(|n| n.as_f64().is_some_and(f64::is_finite))
                })
        }
        "ColorEmotionInput" => crate::base_system::layer_four::color_emotion_system::valid(value),
        "ColorEmotionView" => crate::base_system::layer_four::color_emotion_system::valid_view(value),
        "ViewRequest" => {
            let valid_instances=|rows:&Value,limit:usize|rows.is_null() || rows.as_array().is_some_and(|rows|rows.len()<=limit && rows.iter().all(|r|r["entity"].as_u64().is_some_and(|n|n>0) && crate::base_system::layer_two::player_model_system::math::vector(&r["origin"]).is_ok()));
            valid_instances(&value["water_instances"],16) && crate::base_system::layer_four::appearance_system::projection::valid(value)
                && (value["emotion"].is_null() || value["emotion"].as_u64().is_some_and(|x|x>0))
                && (value["instances"].is_null() || value["instances"].as_array().is_some_and(|rows| {
                    rows.len()<=crate::base_system::layer_four::appearance_system::MAX_SCENE_INSTANCES && rows.iter().all(|r|r["entity"].as_u64().is_some_and(|id|id>0)
                        && crate::base_system::layer_two::player_model_system::math::vector(&r["origin"]).is_ok())
                }))
        }
        "TriangleBatch" => {
            let water_valid=value["water"].is_null() || value["water"].as_array().is_some_and(|rows|rows.len()<=8 && rows.iter().all(crate::water_render_data::valid_packet));
            water_valid &&
            (value["vegetation"].is_null() || crate::vegetation::packet::Scene::from_view(&value["view"]).and_then(|scene|crate::vegetation::packet::Packet::from_geometry(&value["vegetation"],scene)).is_ok())
            && value["entity"].as_u64().is_some_and(|n| n > 0)
                && value["triangles"].as_array().is_some_and(|v| {
                    v.len() <= crate::scene_limits::MAX_SCENE_TRIANGLES
                        && v.iter().all(|t| {
                            t.as_array().is_some_and(|t| {
                                t.len() == 24
                                    && t.iter().all(|n| {
                                        n.as_f64().is_some_and(|n| {
                                            n.is_finite() && (n as f32).is_finite()
                                        })
                                    })
                            })
                        })
                })
        }
        "WindowFrame" => {
            value["closed"].is_boolean()
                && value["drawn"].is_boolean()
                && value["keys"]
                    .as_array()
                    .is_some_and(|k| k.len() == 3 && k.iter().all(Value::is_boolean))
        }
        "CueStatus" => {
            value["active"].is_boolean()
                && (value["visual"].is_null() || check_payload("EntityRef", &value["visual"]))
        }
        "SessionReport" => {
            value["mode"] == "interactive"
                && value["entities_remaining"] == 0
                && value["cues_finished"].is_u64()
                && value["cues_cancelled"].is_u64()
        }
        "JackDiagnostic" => {
            value["mode"] == "jack_connection_diagnostic"
                && matches!(
                    value["status"].as_str(),
                    Some(
                        "library_unavailable"
                            | "api_unavailable"
                            | "connection_failed"
                            | "connected_and_closed"
                            | "close_failed"
                    )
                )
                && value["server_autostart"] == false
                && value["audio_activated"] == false
                && value["audio_produced"] == false
        }
        "RuntimeValidation" => {
            value["mode"] == "runtime_validation"
                && value["contracts_valid"] == true
                && value["graphics_verified"] == false
                && value["audio_verified"] == false
        }
        "NativeInventory" => {
            value["mode"] == "inventory_only"
                && value["backends"].as_array().is_some_and(|b| b.len() == 3)
                && value["jack"].is_object()
        }
        "ProcedureRef" => {
            value.as_object().is_some_and(|v| v.len() == 1)
                && value["procedure"]
                    .as_str()
                    .is_some_and(crate::base_system::layer_two::cue_procedure_system::valid_name)
        }
        "CueDefinition" => {
            crate::base_system::layer_two::cue_procedure_system::validate(value).is_ok()
        }
        "EntitySpec" => crate::base_entity::Entity::from_spec(1, value).is_ok(),
        "EntityData" => {
            value["id"].as_u64().is_some_and(|id| id > 0)
                && crate::base_entity::Entity::from_spec(1, value).is_ok()
        }
        "EntityRef" => value["entity"].as_u64().is_some_and(|id| id > 0),
        "AudicleRef" => value["audicle"].as_u64().is_some_and(|id| id > 0),
        "AudioReport" => {
            value["loopback_verified"] == true
                && value["samples_produced"].as_u64().is_some_and(|n| n > 0)
        }
        "IOSAudioReport" => {
            value["api"] == "Apple RemoteIO"
                && value["completed"] == true
                && value["output_buffer_verified"] == true
                && value["active_callback_discontinuities"] == 0
                && value["samples_produced"].as_u64().is_some_and(|n| n > 0)
        }
        "Presentation" => {
            value["backend"].is_string()
                && value["frames"]
                    .as_u64()
                    .is_some_and(|n| match value.get("end_reason") {
                        None => n > 0,
                        Some(reason) if reason == "frame_limit" => n > 0,
                        Some(reason) if reason == "window_closed" => true,
                        _ => false,
                    })
        }
        "CueResult" => {
            value["mode"] == "cue"
                && (value["stage"] == "Finished"
                    || (value["stage"] == "Cancelled" && value["reason"] == "window_closed"))
                && value["entities_remaining"] == 0
        }
        _ => false,
    }
}

impl Host {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        Self::load_paths(root, &root.join("output"))
    }
    pub(crate) fn load_paths(root: &Path, output: &Path) -> Result<Self, String> {
        let mut mods=crate::modding::Manager::from_root(root)?;
        mods.activate()?;
        let registry = read(root, "BaseSystem/registry.json")?;
        let mut enabled = BTreeMap::new();
        for (name, flag) in object(&registry, "registry")? {
            if !identifier(name) {
                return Err(format!("invalid System name {name}"));
            }
            enabled.insert(
                name.clone(),
                flag.as_bool()
                    .ok_or_else(|| format!("registry {name} must be boolean"))?,
            );
        }
        for (system,capability) in [("LaboratorySystem","laboratory"),("MonasterySystem","monastery"),("MonasteryRenderSystem","monastery"),("NpcSystem","town_npcs")] {
            if !mods.provides(capability){if let Some(flag)=enabled.get_mut(system){*flag=false;}}
        }
        let breaker = read(root, "BaseSystem/circuit_breaker.json")?;
        let breaker_fields = object(&breaker, "circuit breaker")?;
        if breaker_fields.len() != 1 || !breaker_fields.contains_key("halt_execution") {
            return Err("circuit breaker only accepts halt_execution; System toggles belong in registry.json".into());
        }
        let halt = breaker["halt_execution"]
            .as_bool()
            .ok_or("halt_execution must be boolean")?;
        let mut catalog = read(root, "Procedures/functionregistry.json")?;
        if catalog["schema_version"] != 1 {
            return Err("unsupported function registry version".into());
        }
        let mut native_hooks=BTreeMap::new();
        let mut native_contracts=BTreeMap::new();
        let mut native_order=Vec::new();
        for (provider,contract,metadata) in mods.system_extensions()? {
            let system=contract["system"].as_str().ok_or("native System name")?.to_owned();
            if enabled.contains_key(&system){return Err(format!("mod cannot replace registered System {system}"));}
            for name in object(&metadata["functions"],"mod functions")?.keys(){native_hooks.insert(format!("{system}.{name}"),provider.clone());}
            enabled.insert(system.clone(),true);
            catalog["systems"].as_object_mut().ok_or("System catalog")?.insert(system.clone(),metadata);
            native_order.push(format!("{system}.json"));
            native_contracts.insert(system,contract);
        }
        let mut hooks = BTreeMap::new();
        let mut contracts = BTreeMap::new();
        for (system, entry) in object(&catalog["systems"], "function registry systems")? {
            if !identifier(system) || !enabled.contains_key(system) {
                return Err(format!("unregistered System {system}"));
            }
            let layer = entry["layer"]
                .as_u64()
                .filter(|n| (1..=5).contains(n))
                .ok_or("invalid layer")?;
            let contract = if let Some(c)=native_contracts.get(system){c.clone()}else{read(root, &format!("Systems/{system}.json"))?};
            super::intent::validate_contract(system, &contract, &catalog)?;
            if contract["system"] != system.as_str() || contract["layer"] != layer {
                return Err(format!("contract identity/layer mismatch for {system}"));
            }
            let declarations = object(&contract["functions"], "contract functions")?;
            let exports = object(&entry["functions"], "catalog functions")?;
            if declarations.keys().collect::<Vec<_>>() != exports.keys().collect::<Vec<_>>() {
                return Err(format!("catalog/contract function mismatch for {system}"));
            }
            for (name, signature) in exports {
                if !identifier(name) {
                    return Err("invalid function name".into());
                }
                let qualified = format!("{system}.{name}");
                let (actual_layer, input, output) = if native_hooks.contains_key(&qualified) {
                    let input=signature["input"].as_str().ok_or("native input type")?;
                    let output=signature["output"].as_str().ok_or("native output type")?;
                    if !["Unit","GeometryData","ViewRequest"].contains(&input)||!["Unit","GeometryData"].contains(&output){return Err("unsupported native System payload type".into());}
                    (layer,input,output)
                }else{builtin(&qualified).ok_or_else(|| format!("no compiled hook {qualified}"))?};
                if layer != actual_layer
                    || signature["input"] != input
                    || signature["output"] != output
                {
                    return Err(format!("compiled signature mismatch for {qualified}"));
                }
                hooks.insert(
                    qualified,
                    Hook {
                        system: system.clone(),
                        layer,
                        input: input.into(),
                        output: output.into(),
                        calls: names(&declarations[name]["calls"], "calls")?
                            .into_iter()
                            .collect(),
                    },
                );
            }
            if contract.get("update_steps").is_some() || contract.get("cleanup_steps").is_some() {
                return Err(format!(
                    "{system}: System contracts declare init_steps and entry_steps; systems.json defines startup order only"
                ));
            }
            contracts.insert(system.clone(), contract);
        }
        if enabled.len() != contracts.len() {
            return Err("registry and function catalog must list the same Systems".into());
        }
        for (name, hook) in &hooks {
            for target in &hook.calls {
                let _destination = hooks
                    .get(target)
                    .ok_or_else(|| format!("{name} declares unknown hook {target}"))?;
                if target == "AudicleSystem.Create" && hook.layer != 5 {
                    return Err(format!("only layer five may originate Audicles: {name}"));
                }
            }
        }
        let schedule = read(root, "Procedures/systems.json")?;
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        for file in names(&schedule["systems_order"], "systems_order")?.into_iter().chain(native_order) {
            let name = file
                .strip_suffix(".json")
                .filter(|n| identifier(n))
                .ok_or("invalid schedule filename")?;
            if !contracts.contains_key(name) || !seen.insert(name.to_owned()) {
                return Err(format!("unknown/duplicate scheduled System {name}"));
            }
            order.push(name.to_owned());
        }
        if order.len() != contracts.len() {
            return Err("schedule must account for every declared System".into());
        }
        let startup_order: Vec<String> = order.iter().filter(|s| enabled[*s]).cloned().collect();
        let mut startup_steps = Vec::new();
        let mut all_init: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut seen_startup = BTreeSet::new();
        for system in &order {
            for step in names(&contracts[system]["init_steps"], "init_steps")? {
                let name = format!("{system}.{step}");
                let hook = hooks
                    .get(&name)
                    .ok_or_else(|| format!("unknown startup hook {name}"))?;
                if hook.input != "Unit" {
                    return Err(format!("startup hook {name} requires input"));
                }
                if !seen_startup.insert(name.clone()) {
                    return Err(format!("duplicate startup hook {name}"));
                }
                all_init.entry(system.clone()).or_default().push(name.clone());
                if enabled[system] {
                    startup_steps.push(name);
                }
            }
        }
        let mut roots: Vec<String> = all_init.values().flatten().cloned().collect();
        for system in &order {
            let Some(entries) = contracts[system].get("entry_steps") else {
                continue;
            };
            for step in names(entries, "entry_steps")? {
                let name = format!("{system}.{step}");
                if !hooks.contains_key(&name) {
                    return Err(format!("unknown entry hook {name}"));
                }
                {
                    if roots.contains(&name) {
                        return Err(format!("duplicate/automatic entry hook {name}"));
                    }
                    roots.push(name);
                }
            }
        }
        let trace = output.join("execution.jsonl");
        static RUN_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let run_id = format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos(),
            RUN_SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        Ok(Self {
            root: root.into(),
            output: output.into(),
            enabled,
            hooks,
            native_hooks,
            roots,
            startup_steps,
            startup_order,
            startup_attempted: false,
            all_init,
            initialized: BTreeSet::new(),
            initialization_failed: BTreeSet::new(),
            stack: Vec::new(),
            next_id: 1,
            halt,
            trace,
            trace_writer: RefCell::new(None),
            audio_preparation_journal:None,
            run_id,
            mods,
            performance: super::performance_telemetry::Telemetry::new(),
            water: Default::default(),
            vegetation: Default::default(),
            caves: Default::default(),
            workstation: Default::default(),
            workstation_panels: Default::default(),
            security_cameras: Default::default(),
            audio_workstation: Default::default(),
            workstation_export: Default::default(),
            chuck: Default::default(),
            ray_audio: Default::default(),
            environment_audio: Default::default(),
            glasses: Default::default(),
            glasses_render: Default::default(),
            crafting: Default::default(),
            crafting_render: Default::default(),
            phone: Default::default(),
            phone_render: Default::default(),
            theatre: Default::default(),
            audio_visualizer: Default::default(),
            monastery: Default::default(),
            laboratory: Default::default(),
            npcs: Default::default(),
            deer: Default::default(),
            silver_spider_crab: Default::default(),
            birds: Default::default(),
            entities: Default::default(),
            audicles: Default::default(),
            ik:Default::default(),player_rig:Default::default(),
            physics: Default::default(),
            planet: Default::default(),
            terrain_tool: Default::default(),
            shovel_soil: Default::default(),
            shovel: Default::default(),
            ogc: Default::default(),
            graphics: Default::default(),
            scene_preparation:None,
            water_collision_preparation:None,
            color_emotion: Default::default(),
            active_audicle: None,
            audio: Default::default(),
            ios_audio: Default::default(),
            environment: Default::default(),
            flight: Default::default(),
            cue_session: Default::default(),
            verify_session: false,
            audio_enabled: false,
            backend: "OpenGL".into(),
            frame_limit: 120,
            frame_period_us: 0,
            live: false,
            close_after_frames: None,
            game_workstation_benchmark: None,
        })
    }

    fn permitted(&self, target: &str, input: &Value) -> Result<Hook, String> {
        let hook = self.permitted_route(target, input)?;
        if !check_payload(&hook.input, input) {
            return Err(format!("invalid input for {target}"));
        }
        Ok(hook)
    }
    fn permitted_route(&self, target: &str, input: &Value) -> Result<Hook, String> {
        if self.halt {
            return Err("execution halted by circuit breaker".into());
        }
        let hook = self
            .hooks
            .get(target)
            .ok_or_else(|| format!("unknown function {target}"))?;
        if !self.enabled[&hook.system] {
            return Err(format!("disabled System {}", hook.system));
        }
        if let Some((_, caller)) = self.stack.last() {
            if !self.hooks[caller].calls.contains(target) {
                return Err(format!("undeclared call {caller} -> {target}"));
            }
        } else if !self.roots.iter().any(|name| name == target) {
            return Err(format!("Host cannot invoke unscheduled hook {target}"));
        }
        if matches!(
            target,
            "ChuckConnectionSystem.Compile"
                | "GraphicsConnectionSystem.Present"
                | "GraphicsConnectionSystem.SetView"
                | "EntitySystem.Update"
                | "PhysicsSystem.GameStep"
                | "PhysicsSystem.Advance"
                | "PlanetSurfaceSystem.Advance"
                | "PlanetSurfaceSystem.Excavate"
                | "ShovelSoilSystem.Step"
                | "TerrainToolSystem.Cut"
                | "TerrainToolSystem.Advance"
                | "TerrainToolSystem.Prepare"
                | "PlanetSurfaceSystem.MineCell"
                | "PlanetSurfaceSystem.BladeCut"
                | "PlanetSurfaceSystem.RefineMaterial"
                | "PlanetSurfaceSystem.Reset"
                | "JackConnectionSystem.Start"
                | "IOSAudioConnectionSystem.Start"
        ) && !self.audicles.is_go(self.active_audicle)
        {
            return Err("presentation requires an active Audicle in Go".into());
        }
        if target == "EntitySystem.Remove"
            && (self.graphics.prepared_entity() == input["entity"].as_u64()
                || self.graphics.texture_owner() == input["entity"].as_u64()
                || self.graphics.suspended_entity() == input["entity"].as_u64()
                || self.graphics.suspended_texture_owner() == input["entity"].as_u64()
                || self.audio.prepared_entity() == input["entity"].as_u64()
                || self.ios_audio.prepared_entity() == input["entity"].as_u64()
                || self.chuck.prepared_entity() == input["entity"].as_u64())
        {
            return Err("release native resources before removing this Entity".into());
        }
        Ok(hook.clone())
    }

    pub(crate) fn configuration_summary(&self) -> Value {
        json!({"mode":"runtime_validation","contracts_valid":true,"enabled_systems":self.enabled.values().filter(|v|**v).count(),"registered_functions":self.hooks.len(),"graphics_verified":false,"audio_verified":false})
    }
    pub(crate) fn reset_performance(&mut self) {
        self.performance.reset();
    }
    pub(crate) fn set_performance_daw_only(&mut self, enabled: bool) {
        self.performance.daw_only = enabled;
    }
    pub(crate) fn performance_report(&self) -> Value {
        let mut report = self.performance.report();
        report["run_id"] = json!(self.run_id);
        report["generated_at_unix_ms"] = json!(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).ok()
            .map(|time| time.as_millis().min(u64::MAX as u128) as u64));
        report
    }
    pub(crate) fn write_performance_report(&self) -> Result<(), String> {
        if !super::performance_telemetry::enabled() { return Ok(()); }
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let path = self.output.join("performance.json");
        let temporary = self.output.join("performance.json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(&self.performance_report()).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        fs::rename(temporary, path).map_err(|e| e.to_string())?;
        self.performance.report_written();
        Ok(())
    }
    pub(crate) fn validate_runtime(&mut self) -> Result<Value, String> {
        self.invoke("DiagnosticsSystem.ValidateRuntime", Value::Null)
    }
    pub(crate) fn mod_call(&mut self,id:&str,hook:&str,input:&Value)->Result<Value,String>{
        let request=json!({"input":input,"context":{"origin":self.theatre.origin,"output":self.output_root(),"audicle":self.active_audicle}});
        let mut mods=std::mem::take(&mut self.mods);
        let result=mods.call_host(id,hook,&request,(self as *mut Self).cast());
        self.mods=mods;result
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    #[cfg(target_os = "ios")]
    pub(crate) fn execution_run_id(&self) -> &str {
        &self.run_id
    }
    pub(crate) fn output_root(&self) -> &Path {
        &self.output
    }
    pub(crate) fn audio_enabled(&self) -> bool {
        self.audio_enabled
    }
    pub(crate) fn run_av(&mut self, backend: &str, frames: u32) -> Result<(), String> {
        let previous = std::mem::replace(&mut self.audio_enabled, true);
        let result = self.run_triangle(backend, frames);
        self.audio_enabled = previous;
        result
    }
    pub(crate) fn close_after_frames(&self) -> Option<u32> {
        self.close_after_frames
    }
    pub(crate) fn live(&self) -> bool {
        self.live
    }
    pub(crate) fn run_workshop(
        &mut self,
        case: &str,
        backend: &str,
        frames: Option<u32>,
    ) -> Result<(), String> {
        self.backend = backend.into();
        let mut input = json!({"case":case,"scale":1});
        if let Some(n) = frames {
            input["frames"] = json!(n);
        }
        let report = self.invoke("WorkshopSystem.Run", input)?;
        fs::create_dir_all(self.output.join("simulations")).map_err(|e| e.to_string())?;
        fs::write(
            self.output
                .join(format!("simulations/{case}-{backend}.json")),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        println!("{report}");
        Ok(())
    }
    pub(crate) fn run_workstation(&mut self, backend: &str, frames: Option<u32>) -> Result<(),String> {
        if !matches!(backend,"OpenGL"|"Vulkan"|"WebGPU") {return Err("unsupported workstation graphics backend".into());}
        self.backend = backend.into();
        let previous_daw = std::mem::replace(&mut self.performance.daw_only, true);
        let result=self.invoke("AudioWorkstationSystem.Run",json!({"frames":frames}));
        self.performance.daw_only = previous_daw;
        let report=result?;
        fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
        fs::write(self.output.join(format!("workstation-{backend}.json")),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("{report}");
        Ok(())
    }
    pub(crate) fn run_workstation_benchmark(&mut self, backend: &str, frames: u32) -> Result<(), String> {
        if !matches!(backend, "OpenGL" | "Vulkan" | "WebGPU") {
            return Err("unsupported workstation graphics backend".into());
        }
        if !(8..=600).contains(&frames) {
            return Err("workstation benchmark needs 8 through 600 measured frames".into());
        }
        let request = crate::workstation::benchmark::request_from_environment(frames)?;
        self.backend = backend.into();
        let previous_daw = std::mem::replace(&mut self.performance.daw_only, true);
        let result = self.invoke("AudioWorkstationSystem.Run", request);
        self.performance.daw_only = previous_daw;
        let mut failure = result.as_ref().err().cloned();
        let mut report = result.unwrap_or_else(|error| json!({
            "benchmark": {"complete": false, "requested_measured_frames": frames}, "error": error
        }));
        if let Err(error) = crate::workstation::benchmark::augment_gpu(&mut report, &self.output, backend) {
            report["gpu_error"] = json!(error);
            report["benchmark"]["complete"] = json!(false);
            failure.get_or_insert(error);
        }
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        fs::write(self.output.join(format!("workstation-benchmark-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        self.write_performance_report()?;
        println!("{report}");
        if let Some(error) = failure { return Err(error); }
        if report["benchmark"]["complete"] != true {
            return Err("workstation benchmark incomplete; partial report saved".into());
        }
        Ok(())
    }
    pub(crate) fn run_game_workstation_benchmark(&mut self, backend: &str, frames: u32) -> Result<(), String> {
        let mut request = crate::workstation::benchmark::request_from_environment(frames)?;
        if let Ok(kind) = std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_EYEWEAR") {
            if !matches!(kind.as_str(), "ar" | "vr") {
                return Err("benchmark eyewear must be ar or vr".into());
            }
            request["eyewear"] = json!(kind);
        }
        if std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_OBSERVER_MOTION").as_deref() == Ok("1") {
            if request["eyewear"] != "vr" {return Err("benchmark observer motion requires VR".into());}
            request["benchmark"]["observer_motion"] = json!(true);
        }
        if request["benchmark"]["audio"] == true {
            return Err("combined benchmark uses the real game audio session; standalone MIDI fixture is incompatible".into());
        }
        request["benchmark"]["require_embedded_passthrough"] = json!(true);
        if std::env::var("WORMWOOD_PIANO_CHECK_CAMERA").as_deref()==Ok("1"){request["diagnostic_bubbles"]=json!(true);}
        self.backend = backend.into();
        self.audio_enabled = true;
        let previous = self.game_workstation_benchmark.replace(request);
        let result = self.invoke("ApplicationSystem.Run", json!({"procedure":"planet_surface"}));
        self.game_workstation_benchmark = previous;
        let report = result.unwrap_or_else(|error|json!({"error":error,"complete":false}));
        fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
        fs::write(self.output.join(format!("game-workstation-benchmark-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("{report}");
        if report["combined_workstation"]["benchmark"]["complete"] != true || report["entities_remaining"] != 0 {
            return Err("combined game/workstation benchmark incomplete; report preserved".into());
        }
        Ok(())
    }
    pub(crate) fn check_bubble_notes(&mut self, backend:&str) -> Result<(),String> {
        if !matches!(backend,"OpenGL"|"Vulkan"|"WebGPU") {return Err("unsupported workstation graphics backend".into());}
        self.backend=backend.into();
        let report=self.invoke("AudioWorkstationSystem.Run",json!({"frames":4,"diagnostic_bubbles":true}))?;
        fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
        fs::write(self.output.join(format!("bubble-piano-roll-{backend}.json")),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("{report}");
        if report["frames"]!=4 || report["end_reason"]!="frame_limit"
            || report["piano_roll"]["open"]!=true || !report["editor_error"].is_null()
            || report["entities_remaining"]!=0 {
            return Err("bubble diagnostic incomplete; see its saved report".into());
        }
        Ok(())
    }
    pub(crate) fn check_workstation_controls(&mut self, backend: &str) -> Result<(), String> {
        if !matches!(backend, "OpenGL" | "Vulkan" | "WebGPU") {
            return Err("unsupported workstation graphics backend".into());
        }
        self.backend = backend.into();
        let result = self.invoke("AudioWorkstationSystem.Run", json!({
            "frames": 4, "diagnostic_controls": true
        }));
        let mut failure = result.as_ref().err().cloned();
        let mut report = result.unwrap_or_else(|error| json!({
            "mode": "workstation_controls_diagnostic", "error": error
        }));
        // These are the actual submitted poses, after the plugin page has had
        // its opportunity to hide the six authored meter/fader instances.
        let capture = (|| -> Result<Vec<[f64; 4]>, String> {
            // An incomplete run must not reuse a previous capture in this directory.
            if report["frames"] != 4 || report["end_reason"] != "frame_limit" {
                return Err("control diagnostic did not render its final capture frame".into());
            }
            let frame: Value = serde_json::from_slice(
                &fs::read(self.output.join("workstation-optical-frame.json")).map_err(|e| e.to_string())?
            ).map_err(|e| e.to_string())?;
            (4..10).map(|instance| {
                serde_json::from_value(frame["rows"][192 + 2 * instance].clone())
            }).collect::<Result<_, _>>().map_err(|e| e.to_string())
        })();
        let poses = capture.unwrap_or_else(|error| {
            report["capture_error"] = json!(error);
            failure.get_or_insert(error);
            Vec::new()
        });
        let visible = poses.len() == 6
            && poses.iter().all(|pose| pose.iter().all(|v| v.is_finite()) && pose[3] > 0.);
        report["mixer_control_instances"] = json!([4, 5, 6, 7, 8, 9]);
        report["mixer_control_poses"] = json!(poses);
        report["mixer_controls_visible"] = json!(visible);
        report["complete"] = json!(failure.is_none() && report["frames"] == 4 && report["end_reason"] == "frame_limit"
            && report["controls_mixer"] == true && report["controls_settled"] == true
            && report["editor_error"].is_null() && report["arrangement_error"].is_null()
            && report["entities_remaining"] == 0 && visible);
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        fs::write(self.output.join(format!("workstation-controls-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        println!("{report}");
        if report["complete"] != true {
            return Err("workstation control diagnostic incomplete; see its saved report".into());
        }
        Ok(())
    }
    pub(crate) fn check_arrangement(&mut self, backend:&str) -> Result<(),String> {
        if !matches!(backend,"OpenGL"|"Vulkan"|"WebGPU") {return Err("unsupported workstation graphics backend".into());}
        self.backend=backend.into();
        let report=self.invoke("AudioWorkstationSystem.Run",json!({"frames":4,"diagnostic_arrangement":true}))?;
        fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
        fs::write(self.output.join(format!("arrangement-{backend}.json")),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("{report}");
        if !report["arrangement_error"].is_null() || report["entities_remaining"]!=0 {
            return Err("arrangement diagnostic failed; see its saved report".into());
        }
        Ok(())
    }
    pub(crate) fn check_phone(&mut self, backend:&str) -> Result<(),String> {
        if !matches!(backend,"OpenGL"|"Vulkan"|"WebGPU") {return Err("unsupported phone graphics backend".into());}
        self.backend=backend.into();
        let previous=std::mem::replace(&mut self.audio_enabled,true);
        let result=self.invoke("PhoneSystem.Check",Value::Null);
        self.audio_enabled=previous;
        let report=result?;
        println!("{report}");
        if report["complete"]!=true || report["entities_remaining"]!=report["initial_entities"] {
            return Err("phone diagnostic incomplete; see its saved report".into());
        }
        Ok(())
    }
    pub(crate) fn check_audio_spheres(&mut self, backend:&str) -> Result<(),String> {
        if !matches!(backend,"OpenGL"|"Vulkan"|"WebGPU") {return Err("unsupported workstation graphics backend".into());}
        self.backend=backend.into();
        let report=self.invoke("AudioWorkstationSystem.Run",json!({"frames":4,"diagnostic_spheres":true}))?;
        fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
        fs::write(self.output.join(format!("audio-spheres-{backend}.json")),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("{report}");
        if !report["spatial_error"].is_null() || !report["spatial_mix"]["sources"].as_array()
            .is_some_and(|sources|sources.iter().filter(|s|s["available"]==true && s["fresh"]==true).count()>=3) {
            return Err("audio sphere diagnostic did not present its PCM observations".into());
        }
        Ok(())
    }
    pub(crate) fn run_simulation(&mut self, case: &str, frames: u32) -> Result<(), String> {
        let report = self.invoke(
            "WorkshopSystem.Benchmark",
            json!({"case":case,"scale":1,"frames":frames}),
        )?;
        fs::create_dir_all(self.output.join("simulations")).map_err(|e| e.to_string())?;
        fs::write(
            self.output.join(format!("simulations/{case}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("{report}");
        if report["physics"]["halted"] == true {
            Err("simulation halted by validation".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn run_interactive(
        &mut self,
        procedure: &str,
        backend: &str,
        verify: bool,
    ) -> Result<(), String> {
        if !cfg!(any(target_os = "macos", target_os = "ios")) {
            return Err("interactive sessions currently enabled for macOS and iOS".into());
        }
        self.backend = backend.into();
        self.audio_enabled = true;
        self.verify_session = verify;
        let report = self.invoke("ApplicationSystem.Run", json!({"procedure":procedure}))?;
        fs::write(
            self.output.join(format!("interactive-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("{report}");
        Ok(())
    }
    pub(crate) fn run_interactive_bounded(&mut self,procedure:&str,backend:&str,verify:bool,frames:u32)->Result<(),String> {
        if !(1..=3600).contains(&frames) {return Err("interactive diagnostic close count must be 1..3600".into());}
        let previous=std::mem::replace(&mut self.close_after_frames,Some(frames));
        let result=self.run_interactive(procedure,backend,verify);
        self.close_after_frames=previous;
        result?;
        if verify && frames>=480 && procedure=="planet_surface" && self.system_enabled("EnvironmentAudioSystem") {
            let report:Value=serde_json::from_slice(&fs::read(self.output.join(format!("interactive-{backend}.json"))).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            let audio=&report["environment_audio"];
            if !audio["simulated_seconds"].as_f64().is_some_and(|seconds|seconds>=7.9)
                || !audio["source_count_max"].as_u64().is_some_and(|count|(4..=17).contains(&count))
                || !audio["native"]["stream_output_peak"].as_f64().is_some_and(|peak|peak>1e-6)
                || audio["vm"]["render_errors"]!=0 || report["entities_remaining"]!=0 {
                return Err(format!("enabled environment audio verification failed: {audio}"));
            }
        }
        Ok(())
    }
    pub(crate) fn run_live(
        &mut self,
        procedure: &str,
        backend: &str,
        close_after: Option<u32>,
    ) -> Result<(), String> {
        if !cfg!(target_os = "macos") {
            return Err("live cue mode is currently enabled for macOS".into());
        }
        if close_after.is_some_and(|n| !(1..=600).contains(&n)) {
            return Err("diagnostic close count must be 1..600".into());
        }
        let previous_close = std::mem::replace(&mut self.close_after_frames, close_after);
        let previous_live = std::mem::replace(&mut self.live, true);
        let previous_period = std::mem::replace(&mut self.frame_period_us, 16_667);
        let result = self.run_procedure(procedure, backend, 120);
        self.close_after_frames = previous_close;
        self.live = previous_live;
        self.frame_period_us = previous_period;
        result
    }
    pub(crate) fn frame_period_us(&self) -> u64 {
        self.frame_period_us
    }
    pub(crate) fn run_soak(
        &mut self,
        backend: &str,
        seconds: u32,
        repeats: u32,
    ) -> Result<(), String> {
        if !(1..=10).contains(&seconds) || !(1..=20).contains(&repeats) {
            return Err("soak requires seconds 1..10 and repeats 1..20".into());
        }
        let previous = std::mem::replace(&mut self.frame_period_us, 16_667);
        let mut runs = Vec::new();
        let result = (|| {
            for _ in 0..repeats {
                self.run_av(backend, seconds * 60)?;
                let bytes = fs::read(self.output.join(format!("av-{backend}.json")))
                    .map_err(|e| e.to_string())?;
                let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if self.entities.len() != 0
                    || self.active_audicle.is_some()
                    || self.graphics.prepared_entity().is_some()
                    || self.graphics.texture_owner().is_some()
                    || self.graphics.suspended_entity().is_some()
                    || self.audio.prepared_entity().is_some()
                    || self.ios_audio.prepared_entity().is_some()
                {
                    return Err(
                        "soak retained active native/Entity/Audicle ownership between cues".into(),
                    );
                }
                let finished = report["stage"] == "Finished";
                runs.push(report);
                if !finished {
                    return Err("soak cue was cancelled".into());
                }
            }
            Ok(())
        })();
        self.frame_period_us = previous;
        let report = json!({"mode":"native_callback_render_soak","backend":backend,"seconds_per_cue":seconds,"frame_period_us":16_667,"requested_repeats":repeats,"same_process":true,"execution_run_id":self.run_id,"passed":result.is_ok(),"runs":runs,"error":result.as_ref().err()});
        fs::write(
            self.output.join(format!("soak-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        result
    }
    pub(crate) fn backend(&self) -> &str {
        &self.backend
    }
    pub(crate) fn frame_limit(&self) -> u32 {
        self.frame_limit
    }

    pub(crate) fn emit(&self, mut event: Value) -> Result<(), String> {
        let (_, caller) = self
            .stack
            .last()
            .ok_or("lifecycle event requires an active registered hook")?;
        let kind = event["event"].as_str().ok_or("event kind required")?;
        let allowed = match kind {
            "entity_created" => caller == "EntitySystem.Create",
            "entity_updated" => caller == "EntitySystem.Update",
            "entity_removed" => caller == "EntitySystem.Remove",
            "audicle_stage" | "audicle_cancelled" | "audicle_failed" => {
                caller.starts_with("AudicleSystem.") && self.hooks[caller].layer == 5
            }
            "audio_prepared" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Prepare" | "IOSAudioConnectionSystem.Prepare"
            ),
            "audio_started" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Start" | "IOSAudioConnectionSystem.Start"
            ),
            "audio_completed" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Stop" | "IOSAudioConnectionSystem.Stop"
            ),
            "audio_released" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Release" | "IOSAudioConnectionSystem.Release"
            ),
            "physics_configured" => caller == "PhysicsSystem.Configure",
            "terrain_tool_cut" => caller == "TerrainToolSystem.Cut",
            "terrain_cut_rejected" => caller == "ShovelSystem.Step",
            "planet_excavated" => caller == "PlanetSurfaceSystem.Excavate",
            "planet_material_step" => caller == "PlanetSurfaceSystem.Advance",
            "physics_step" => caller == "PhysicsSystem.Advance",
            "environment_step" => caller == "PlanetNavigationSystem.Step",
            "water_visibility" => caller == "WaterRenderSystem.Update",
            "flight_step" => caller == "FlightSystem.Step",
            "view_updated" | "scene_rejected" => caller == "GraphicsConnectionSystem.SetView",
            "window_pumped" => caller == "GraphicsConnectionSystem.Pump",
            "panel_input_applied" => matches!(caller.as_str(), "AudioWorkstationSystem.Run" | "AudioWorkstationSystem.RunEmbedded"),
            "application_input" | "application_audio_error" => caller == "ApplicationSystem.Run",
            "audio_route_changed" => matches!(caller.as_str(),"JackConnectionSystem.RouteChanged"|"IOSAudioConnectionSystem.RouteChanged"),
            "procedure_loaded" => caller == "CueProcedureSystem.Load",
            "presentation_ended" => caller == "GraphicsConnectionSystem.Present",
            "native_prepared" => caller == "GraphicsConnectionSystem.Prepare",
            "native_released" => caller == "GraphicsConnectionSystem.Release",
            "frame_presented" => caller == "GraphicsConnectionSystem.Present",
            _ => false,
        };
        if !allowed {
            return Err(format!("{caller} cannot emit {kind}"));
        }
        event["function"] = json!(caller);
        event["invocation"] = json!(self.stack.last().map(|(id, _)| id));
        event["audicle"] = json!(self.active_audicle);
        self.record(event)
    }

    pub(crate) fn run_procedure(
        &mut self,
        procedure: &str,
        backend: &str,
        frames: u32,
    ) -> Result<(), String> {
        let previous = std::mem::replace(&mut self.audio_enabled, true);
        let result = self.run_named_cue(procedure, backend, frames);
        self.audio_enabled = previous;
        result
    }
    pub(crate) fn run_triangle(&mut self, backend: &str, frames: u32) -> Result<(), String> {
        self.run_named_cue("hello_triangle", backend, frames)
    }
    fn run_named_cue(&mut self, procedure: &str, backend: &str, frames: u32) -> Result<(), String> {
        if !matches!(backend, "OpenGL" | "Vulkan" | "WebGPU") {
            return Err("backend must be OpenGL, Vulkan, or WebGPU".into());
        }
        if !(1..=600).contains(&frames) {
            return Err("frame limit must be 1..600".into());
        }
        self.backend = backend.into();
        self.frame_limit = frames;
        let result = self.invoke("CueSystem.Run", json!({"procedure":procedure}));
        let report = match &result {
            Ok(value) => value.clone(),
            Err(error) => {
                json!({"mode":"cue","backend":backend,"error":error,"success":false})
            }
        };
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        fs::write(
            self.output.join(format!(
                "{}-{backend}.json",
                if self.live {
                    "live"
                } else if self.audio_enabled {
                    "av"
                } else {
                    "triangle"
                }
            )),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        result.map(|value| println!("{value}"))
    }

    pub(crate) fn invoking_system(&self) -> &str {
        self.stack.iter().rev().nth(1)
            .and_then(|(_, hook)| hook.split_once('.'))
            .map(|(system, _)| system).unwrap_or("Host")
    }

    fn record(&self, mut event: Value) -> Result<(), String> {
        event
            .as_object_mut()
            .ok_or("trace event must be an object")?
            .insert("run_id".into(), json!(self.run_id));
        let mut trace=self.trace_writer.borrow_mut();
        if trace.as_ref().is_none_or(|(path,_)|path!=&self.trace){
            fs::create_dir_all(&self.output).map_err(|e|e.to_string())?;
            *trace=Some((self.trace.clone(),super::trace_output::Writer::open(&self.trace).map_err(|e|e.to_string())?));
        }
        let mut bytes=serde_json::to_vec(&event).map_err(|e|e.to_string())?;bytes.push(b'\n');
        let (_,writer)=trace.as_mut().unwrap();
        writer.write_all(&bytes).map_err(|e|e.to_string())?;
        // Preserve every record and keep live diagnostics current once per frame.
        // The callback never enters Host. Finish/failure flushes report I/O errors.
        let kind = event["event"].as_str().unwrap_or("");
        if kind == "window_pumped" || matches!(kind, "failed" | "rejected" | "native_released" | "audicle_cancelled" | "audicle_failed")
            || (kind == "audicle_stage" && event["stage"] == "Finished")
            || (kind == "finished" && event["parent"].is_null()) {
            writer.flush().map_err(|e| e.to_string())?;
        }
        if kind=="finished" && event["function"]=="ApplicationSystem.Run" && event["parent"].is_null(){
            writer.finish_pending().map_err(|e|e.to_string())?;
        }
        Ok(())
    }

    /// Typed transport for the same registered contact operation. It preserves
    /// route/circuit-breaker checks, owner isolation, tracing and batch validation.
    pub(crate) fn contact_responses(&mut self, positions: &[[f32; 3]], previous: &[[f32; 3]])
        -> Result<crate::base_system::layer_one::ogc_connection_system::ResponseBatch, String>
    {
        let target = "OGCConnectionSystem.Respond";
        let _performance = self.performance.scope(target);
        let started = std::time::Instant::now();
        let id = self.next_id;
        self.next_id += 1;
        let parent = self.stack.last().map(|(id, _)| *id);
        let caller = self.stack.last().map(|(_, n)| n.as_str()).unwrap_or("Host");
        if let Err(error) = self.permitted_route(target, &Value::Null) {
            self.record(json!({"event":"rejected","invocation":id,"parent":parent,"caller":caller,"function":target,"error":error}))?;
            return Err(error);
        }
        let owner = caller.split('.').next().unwrap().to_owned();
        self.record(json!({"event":"started","invocation":id,"parent":parent,"caller":caller,"function":target,"layer":1,"input_type":"ContactResponseRequest","audicle":self.active_audicle}))?;
        self.stack.push((id, target.into()));
        let result = self.ogc.assert_owner(&owner).and_then(|_| self.ogc.respond(positions, previous));
        self.stack.pop();
        self.record(json!({"event":if result.is_ok(){"finished"}else{"failed"},"invocation":id,"parent":parent,"function":target,"layer":1,"output_type":"ContactResponseResult","audicle":self.active_audicle,"elapsed_us":started.elapsed().as_micros() as u64,"error":result.as_ref().err()}))?;
        result
    }

    // Bulk numeric transport uses the same registered route, invocation trace
    // and Audicle lifetime as invoke(). Its Rust signature validates the mesh
    // envelope; the owning System still validates geometry and revisions.
    fn invoke_numeric<T>(&mut self, target: &str, operation: impl FnOnce(&mut Self)->Result<T,String>) -> Result<T,String> {
        self.invoke_numeric_input(target,&Value::Null,operation)
    }
    fn invoke_numeric_input<T>(&mut self, target:&str,input:&Value,operation:impl FnOnce(&mut Self)->Result<T,String>)->Result<T,String> {
        let _performance = self.performance.scope(target);
        let started=std::time::Instant::now();
        let id=self.next_id; self.next_id+=1;
        let parent=self.stack.last().map(|(id,_)|*id);
        let caller=self.stack.last().map(|(_,n)|n.as_str()).unwrap_or("Host");
        let hook=match self.permitted_route(target,input) {
            Ok(hook)=>hook,
            Err(error)=> {
                self.record(json!({"event":"rejected","invocation":id,"parent":parent,"caller":caller,"function":target,"error":error}))?;
                return Err(error);
            }
        };
        self.record(json!({"event":"started","invocation":id,"parent":parent,"caller":caller,"function":target,"layer":hook.layer,"input_type":hook.input,"numeric_batch":true,"audicle":self.active_audicle}))?;
        self.stack.push((id,target.into()));
        let result=operation(self);
        self.stack.pop();
        self.record(json!({"event":if result.is_ok(){"finished"}else{"failed"},"invocation":id,"parent":parent,"function":target,"layer":hook.layer,"output_type":hook.output,"numeric_batch":true,"audicle":self.active_audicle,"elapsed_us":started.elapsed().as_micros() as u64,"error":result.as_ref().err()}))?;
        result
    }
    pub(crate) fn vegetation_scene_snapshot(&mut self)->Result<crate::vegetation::State,String> {
        self.invoke_numeric("VegetationSystem.QueryGeometry",|h|Ok(h.vegetation.clone()))
    }
    pub(crate) fn acoustic_plant_snapshot(&mut self)->Result<crate::vegetation::State,String> {
        self.invoke_numeric("VegetationSystem.QueryWood",|h|Ok(h.vegetation.clone()))
    }
    pub(crate) fn acoustic_capture_inputs(&mut self,input:&Value)->Result<crate::base_system::layer_two::terrain_geometry_system::Capture,String> {
        self.invoke_numeric_input("TerrainGeometrySystem.Capture",input,|h|
            crate::base_system::layer_two::terrain_geometry_system::capture_inputs(h,input))
    }
    pub(crate) fn advance_terrain_audio(&mut self, geometry:&std::sync::Arc<Value>, listener:&Value, sources:&Value, time:f64,cabinets:&[crate::audio::line_array::Cabinet])->Result<Value,String> {
        // Borrow the accepted immutable snapshot. Do not copy thousands of wood
        // segments into a JSON invocation every UI frame. Contract/Audicle trace
        // and the complete acoustic ray budget remain the same.
        self.invoke_numeric("RayTracedAudioSystem.AdvanceTerrain", |h| {
            if cfg!(target_os="macos") && !cfg!(test) && std::env::var("WORMWOOD_GAME_ACOUSTICS_SYNC").as_deref()!=Ok("1") {
                crate::base_system::layer_two::terrain_audio::advance_async(&mut h.ray_audio,geometry,listener,sources,time,cabinets)
            } else {
                crate::base_system::layer_two::terrain_audio::advance_parts(&mut h.ray_audio,geometry,listener,sources,time)
            }
        })
    }
    pub(crate) fn advance_terrain_microphone(&mut self, geometry:&std::sync::Arc<Value>, listener:&Value, sources:&Value, time:f64,cabinets:&[crate::audio::line_array::Cabinet])->Result<Value,String> {
        self.invoke_numeric("RayTracedAudioSystem.AdvanceMicrophone", |h| {
            let state=h.ray_audio.microphone_terrain.get_or_insert_with(||Box::new(Default::default()));
            let result=if cfg!(target_os="macos") && !cfg!(test) && std::env::var("WORMWOOD_GAME_ACOUSTICS_SYNC").as_deref()!=Ok("1") {
                crate::base_system::layer_two::terrain_audio::advance_async(state,geometry,listener,sources,time,cabinets)
            } else {
                crate::base_system::layer_two::terrain_audio::advance_parts(state,geometry,listener,sources,time)
            }?;
            h.ray_audio.microphone.responses=state.engine.responses.clone();
            h.ray_audio.microphone.debug=state.engine.debug.clone();
            h.ray_audio.microphone.version=state.engine.version;
            Ok(result)
        })
    }
    pub(crate) fn workstation_snapshot_shared(&mut self)->Result<crate::workstation::SharedSnapshot,String> {
        // Same registered Snapshot permission, caller trace and Audicle owner.
        // Immutable sharing avoids constructing/copying note arrays on each Step.
        self.invoke_numeric("WorkstationDataSystem.Snapshot",|h|h.workstation.shared_snapshot())
    }
    pub(crate) fn material_collision_vertices(&mut self)->Result<Vec<[f64;3]>,String> {
        self.invoke_numeric("PhysicsSystem.GameCollision",|h|Ok(h.physics.game_collision_vertices()))
    }
    pub(crate) fn vegetation_geometry(&mut self, input:&Value)->Result<crate::vegetation::packet::Geometry,String> {
        self.invoke_numeric_input("VegetationSystem.QueryGeometry",input,|h|
            crate::base_system::layer_two::vegetation_system::geometry_numeric(&h.vegetation,input))
    }
    pub(crate) fn theatre_geometry(&mut self,input:&Value)->Result<crate::base_system::layer_two::theatre_system::Mesh,String> {
        self.invoke_numeric_input("TheatreSystem.Mesh",input,|h|
            crate::base_system::layer_two::theatre_system::mesh(&h.theatre,input))
    }
    pub(crate) fn glasses_geometry(&mut self,input:&Value)->Result<Vec<[f32;24]>,String> {
        self.invoke_numeric_input("GlassesRenderSystem.Frame",input,|h|
            crate::base_system::layer_four::glasses_render_system::frame(h,input))
    }
    pub(crate) fn monastery_geometry(&mut self,input:&Value)->Result<Vec<[f32;24]>,String> {
        let parent=self.active_audicle;
        if let Some(id)=self.monastery.audicle(){self.active_audicle=Some(id);}
        let result=self.invoke_numeric_input("MonasteryRenderSystem.Frame",input,|h|crate::base_system::layer_four::monastery_render_system::frame(h,input));
        self.active_audicle=parent;result
    }
    pub(crate) fn crafting_geometry(&mut self,input:&Value)->Result<Vec<[f32;24]>,String> {
        self.invoke_numeric("CraftingRenderSystem.Frame",|h|crate::base_system::layer_four::crafting_render_system::frame(h,input))
    }
    pub(crate) fn phone_geometry(&mut self,input:&Value)->Result<Vec<[f32;24]>,String> {
        self.invoke_numeric_input("PhoneRenderSystem.Frame",input,|h|
            crate::base_system::layer_four::phone_render_system::frame(h,input))
    }
    pub(crate) fn audio_ray_geometry(&mut self,input:&Value)->Result<Vec<[f32;24]>,String> {
        self.invoke_numeric_input("AudioRayVisualizerSystem.Frame",input,|h|
            crate::base_system::layer_four::audio_ray_visualizer_system::frame(h,input))
    }
    pub(crate) fn camera_scene(&mut self,input:&Value)->Result<crate::base_system::layer_four::appearance_system::Scene,String> {
        self.invoke_numeric_input("AppearanceSystem.View",input,|h|crate::base_system::layer_four::appearance_system::scene(h,input))
    }
    pub(crate) fn reuse_workstation_scene(&mut self,entity:u64,rows:&[[f32;4]],capture:bool,overlay:&[[f32;24]])->Result<Value,String>{
        let input=json!({"entity":entity,"capture":capture,"audio_overlay":overlay,"reuse_world":true,"world_backdrop":true});
        self.invoke_numeric_input("GraphicsConnectionSystem.SetView",&input,|h|
            crate::base_system::layer_one::graphics_connection_system::set_workstation_scene_numeric(h,&input,&[],None,rows,&[]))
    }
    pub(crate) fn set_workstation_scene(&mut self,entity:u64,rows:&[[f32;4]],capture:bool,scene:crate::base_system::layer_four::appearance_system::Scene,overlay:&[[f32;24]])->Result<Value,String> {
        let input=json!({"entity":entity,"capture":capture,"audio_overlay":overlay,"view":scene.view,"world_backdrop":true});
        self.invoke_numeric_input("GraphicsConnectionSystem.SetView",&input,|h|
            crate::base_system::layer_one::graphics_connection_system::set_workstation_scene_numeric(h,&input,&scene.triangles,scene.vegetation,rows,&scene.water))
    }
    pub(crate) fn set_scene_view(&mut self, input:&Value, triangles:&[[f32;24]], plants:Option<crate::vegetation::packet::Packet>)->Result<Value,String> {
        self.invoke_numeric_input("GraphicsConnectionSystem.SetView",input,|h|
            crate::base_system::layer_one::graphics_connection_system::set_view_numeric(h,input,&triangles,plants))
    }
    pub(crate) fn set_scene_view_water(&mut self,input:&Value,triangles:&[[f32;24]],plants:Option<crate::vegetation::packet::Packet>,water:&[crate::water::presentation::Packet])->Result<Value,String>{
        self.invoke_numeric_input("GraphicsConnectionSystem.SetView",input,|h|crate::base_system::layer_one::graphics_connection_system::set_view_water_numeric(h,input,triangles,plants,water))
    }
    pub(crate) fn set_workstation_view(&mut self, entity:u64, triangles:&[[f32;24]], rows:&[[f32;4]], capture:bool)->Result<Value,String> {
        self.set_workstation_view_material_eye(entity,triangles,rows,capture,Value::Null)
    }
    pub(crate) fn set_workstation_view_material_eye(&mut self, entity:u64, triangles:&[[f32;24]], rows:&[[f32;4]], capture:bool, table_eye:Value)->Result<Value,String>{
        let input=json!({"entity":entity,"capture":capture,"table_eye":table_eye});
        self.invoke_numeric_input("GraphicsConnectionSystem.SetView",&input,|h|
            crate::base_system::layer_one::graphics_connection_system::set_workstation_view_numeric(h,&input,triangles,rows))
    }
    pub(crate) fn material_vertices(&mut self,eye:[f64;3],forward:[f64;3],up:[f64;3],aspect:f64)->Result<Vec<[f64;6]>,String> {
        self.invoke_numeric("PhysicsSystem.GameFrame",|h|h.physics.game_vertices(eye,forward,up,aspect))
    }
    pub(crate) fn entity_snapshot(&mut self,id:u64)->Result<crate::base_entity::Entity,String> {
        self.invoke_numeric_input("EntitySystem.Get",&json!({"entity":id}),|h|h.entities.snapshot(id))
    }
    pub(crate) fn entity_shared_snapshot(&mut self,id:u64)->Result<std::sync::Arc<crate::base_entity::Entity>,String> {
        self.invoke_numeric_input("EntitySystem.Get",&json!({"entity":id}),|h|h.entities.shared_snapshot(id))
    }
    pub(crate) fn update_player_skin(&mut self,id:u64,vertices:Vec<[f32;6]>)->Result<(),String>{
        self.invoke_numeric_input("EntitySystem.Update",&json!({"entity":id}),|h|{
            let count=vertices.len();h.entities.update_player_vertices(id,vertices)?;
            h.emit(json!({"event":"entity_updated","entity":id,"vertices":count,"numeric_batch":true}))
        })
    }
    pub(crate) fn update_material_entity(&mut self,id:u64,procedural:Value,vertices:Vec<[f64;6]>)->Result<(),String> {
        self.invoke_numeric_input("EntitySystem.Update",&json!({"entity":id}),|h| {
            let count=vertices.len();
            h.entities.update_material(id,procedural,vertices)?;
            h.emit(json!({"event":"entity_updated","entity":id,"procedural_debris_vertices":count,"numeric_batch":true}))
        })
    }
    pub(crate) fn surface_heights_numeric(&mut self,positions:&[[f64;3]],camera:[f64;3])->Result<Vec<f64>,String>{
        self.invoke_numeric_input("PlanetSurfaceSystem.Height",&json!({"count":positions.len()}),|h|crate::physics::planet::heights_numeric(h,positions,camera))
    }
    pub(crate) fn cave_supports_numeric(&mut self,positions:&[[f64;3]],eye:f64,edits:&[Value])->Result<Vec<Option<f64>>,String>{
        self.invoke_numeric_input("CaveSystem.Support",&json!({"count":positions.len(),"eye_height":eye}),|h|crate::base_system::layer_two::cave_system::supports_numeric(&mut h.caves,positions,eye,edits))
    }
    pub(crate) fn water_surface_numeric(&mut self,input:&Value)->Result<crate::water::runtime::Surface,String>{
        self.invoke_numeric_input("TerrainSurfaceHydrologySystem.SurfaceMesh",input,|h|crate::water::runtime::surface_numeric(h,input))
    }
    pub(crate) fn publish_water_entity(&mut self,existing:Option<u64>,packet:std::sync::Arc<crate::water::presentation::Packet>)->Result<u64,String>{
        let target=if existing.is_some(){"EntitySystem.Update"}else{"EntitySystem.Create"};
        self.invoke_numeric_input(target,&json!({"entity":existing}),|h|{
            let owner=h.invoking_system().to_owned();let count=packet.rows.len();
            let id=h.entities.publish_water(existing,packet,owner)?;
            h.emit(json!({"event":if existing.is_some(){"entity_updated"}else{"entity_created"},"entity":id,"vertices":count,"numeric_batch":true}))?;Ok(id)
        })
    }
    pub(crate) fn water_collision_snapshot(&mut self, position:[f64;3])->Result<crate::water::types::Snapshot,String> {
        self.invoke_numeric("PlanetSurfaceSystem.CollisionSnapshot",|h|{
            if cfg!(target_os="macos")&&!cfg!(test)&&std::env::var("WORMWOOD_GAME_WATER_COLLISION_SYNC").as_deref()!=Ok("1"){
                crate::physics::planet::collision_async(h,position)
            }else{crate::physics::planet::collision_numeric(h,position)}
        })
    }
    pub(crate) fn water_contact_snapshot(&mut self) -> Result<Option<Box<crate::water::hydrology::State>>,String> {
        // Same contracted Snapshot route; immutable numeric collision and its
        // already built spatial index are copied, never encoded/rebuilt via JSON.
        self.invoke_numeric("TerrainSurfaceHydrologySystem.Snapshot", |h| {
            if h.water.hydrology.snapshot.is_none() { return Ok(None); }
            let mut contact=h.water.hydrology.clone();
            contact.bodies=crate::water::bodies::State {
                sources:contact.bodies.sources,
                ..Default::default()
            };
            Ok(Some(Box::new(contact)))
        })
    }
    pub(crate) fn update_water_collision(&mut self, snapshot:crate::water::types::Snapshot)->Result<Value,String> {
        self.invoke_numeric("TerrainSurfaceHydrologySystem.Update",|h|crate::water::runtime::update_numeric(h,snapshot))
    }

    pub(crate) fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String> {
        let _performance = self.performance.scope(target);
        let started = std::time::Instant::now();
        let id = self.next_id;
        self.next_id += 1;
        let parent = self.stack.last().map(|(id, _)| *id);
        let caller = self
            .stack
            .last()
            .map(|(_, name)| name.as_str())
            .unwrap_or("Host");
        let hook = match self.permitted(target, &input) {
            Ok(hook) => hook,
            Err(error) => {
                self.record(json!({"event": "rejected", "invocation": id, "parent": parent, "caller": caller, "function": target, "error": error}))?;
                return Err(error);
            }
        };
        self.record(
            json!({"event": "started", "invocation": id, "parent": parent, "caller": caller,
            "function": target, "layer": hook.layer, "input_type": hook.input, "audicle": self.active_audicle}),
        )?;
        let event_audicle = self.active_audicle;
        self.stack.push((id, target.into()));
        let result = if let Some(provider)=self.native_hooks.get(target).cloned(){self.mod_call(&provider,target,&input)}else{match target {
            "CraftingSystem.Configure" | "CraftingSystem.Close" | "CraftingSystem.Status" | "CraftingSystem.Input" | "CraftingSystem.Frame" | "CraftingSystem.Grip" => crate::base_system::layer_five::crafting_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "CraftingRenderSystem.Load" | "CraftingRenderSystem.Frame" => crate::base_system::layer_four::crafting_render_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "PhoneSystem.Configure" | "PhoneSystem.Input" | "PhoneSystem.Grip" | "PhoneSystem.Frame" | "PhoneSystem.Close" | "PhoneSystem.Status" | "PhoneSystem.Check" | "PhoneSystem.CheckScene" => crate::base_system::layer_five::phone_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "GlassesSystem.Configure" | "GlassesSystem.Input" | "GlassesSystem.Grip" | "GlassesSystem.Status" | "GlassesSystem.Close" => crate::base_system::layer_five::glasses_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "GlassesRenderSystem.Load" | "GlassesRenderSystem.Frame" => crate::base_system::layer_four::glasses_render_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "PhoneRenderSystem.Load" | "PhoneRenderSystem.Frame" => crate::base_system::layer_four::phone_render_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "TheatreSystem.ChairInput" | "TheatreSystem.Configure" | "TheatreSystem.Status" | "TheatreSystem.ResolveBody" | "TheatreSystem.Trace" | "TheatreSystem.Mesh" | "TheatreSystem.AcousticGeometry" => crate::base_system::layer_two::theatre_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "AudioRayVisualizerSystem.Frame" | "AudioRayVisualizerSystem.Clear" => crate::base_system::layer_four::audio_ray_visualizer_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "PluginConnectionSystem.Status" | "PluginConnectionSystem.Command" | "PluginConnectionSystem.Menu" => crate::base_system::layer_one::plugin_connection_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "WorkstationPanelSystem.Suspend" | "WorkstationPanelSystem.Load" | "WorkstationPanelSystem.View" | "WorkstationPanelSystem.Release" => crate::base_system::layer_four::workstation_panel_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "SecurityCameraSystem.View" | "SecurityCameraSystem.Status" => crate::base_system::layer_five::security_camera_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "AudioWorkstationSystem.Run" => crate::base_system::layer_five::audio_workstation_system::run(self,&input),
            "AudioWorkstationSystem.RunEmbedded" => crate::base_system::layer_five::audio_workstation_system::run_embedded(self,&input),
            "AudioWorkstationSystem.Suspend" | "AudioWorkstationSystem.Resume" => crate::base_system::layer_five::audio_workstation_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "GraphicsConnectionSystem.ParkWorkstation" | "GraphicsConnectionSystem.FocusWorkstation" | "GraphicsConnectionSystem.PrewarmWorkstation" | "GraphicsConnectionSystem.Suspend" | "GraphicsConnectionSystem.Resume" => crate::base_system::layer_one::graphics_connection_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "AudioWorkstationSystem.PrimeWorld" | "AudioWorkstationSystem.Background" | "AudioWorkstationSystem.Prewarm" | "AudioWorkstationSystem.PreparePlugins" | "AudioWorkstationSystem.Begin" | "AudioWorkstationSystem.Step" | "AudioWorkstationSystem.SetPanel" | "AudioWorkstationSystem.DrumRack" | "AudioWorkstationSystem.OpenPianoRoll" | "AudioWorkstationSystem.ComputerKeyboard" | "AudioWorkstationSystem.Close" | "AudioWorkstationSystem.Status" => crate::base_system::layer_five::audio_workstation_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "WorkstationExportSystem.Open" | "WorkstationExportSystem.Begin" | "WorkstationExportSystem.Step" | "WorkstationExportSystem.Pump" | "WorkstationExportSystem.Status" | "WorkstationExportSystem.Cancel" | "WorkstationExportSystem.Close" => crate::base_system::layer_five::workstation_export_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "WorkstationDataSystem.Undo" | "WorkstationDataSystem.Redo" | "WorkstationDataSystem.UndoStatus" | "WorkstationDataSystem.BeginUndoTransaction" | "WorkstationDataSystem.CommitUndoTransaction" | "WorkstationDataSystem.CancelUndoTransaction" | "WorkstationDataSystem.ClearUndo" |
            "WorkstationDataSystem.TrackFreezeBegin" | "WorkstationDataSystem.TrackFreezeStep" | "WorkstationDataSystem.TrackFreezeAccept" | "WorkstationDataSystem.TrackFreezeCancel" | "WorkstationDataSystem.TrackFreezeFlatten" | "WorkstationDataSystem.TrackFreezeStatus" |
            "WorkstationDataSystem.ExportBegin" | "WorkstationDataSystem.ExportStep" | "WorkstationDataSystem.ExportCancel" | "WorkstationDataSystem.ExportStatus" |
            "WorkstationDataSystem.BeginRetrospectiveAudio" | "WorkstationDataSystem.AppendRetrospectiveAudio" | "WorkstationDataSystem.CommitRetrospectiveAudio" | "WorkstationDataSystem.AbortRetrospectiveAudio" | "WorkstationDataSystem.InsertRetrospectiveMidi" | "WorkstationDataSystem.UndoRetrospective" | "WorkstationDataSystem.RedoRetrospective" | "WorkstationDataSystem.Snapshot" | "WorkstationDataSystem.CaptureStatus" | "WorkstationDataSystem.DrumRack" | "WorkstationDataSystem.PutBus" | "WorkstationDataSystem.SetBus" | "WorkstationDataSystem.RemoveBus" | "WorkstationDataSystem.PutTrack" | "WorkstationDataSystem.RemoveTrack" |
            "WorkstationDataSystem.PutClip" | "WorkstationDataSystem.CreateRangeClips" | "WorkstationDataSystem.RemoveClip" |
            "WorkstationDataSystem.TrimAudioClip" | "WorkstationDataSystem.SetClipGain" | "WorkstationDataSystem.SetClipRouting" | "WorkstationDataSystem.SplitClip" | "WorkstationDataSystem.SetClipFades" | "WorkstationDataSystem.MoveClip" | "WorkstationDataSystem.RebaseTimeline" | "WorkstationDataSystem.SetMusicalTime" | "WorkstationDataSystem.SetMidiTempo" |
            "WorkstationDataSystem.SnapMidi" | "WorkstationDataSystem.PianoRoll" |
            "WorkstationDataSystem.SetTrackMix" | "WorkstationDataSystem.SetCcTarget" |
            "WorkstationDataSystem.ReorderTracks" | "WorkstationDataSystem.PutSource" | "WorkstationDataSystem.ImportAudio" |
            "WorkstationDataSystem.SnapClip" | "WorkstationDataSystem.RenderRange" | "WorkstationDataSystem.CaptureInput" | "WorkstationDataSystem.CycleMidiTake" | "WorkstationDataSystem.AudioPlaylist" |
        "WorkstationDataSystem.SetMusicalTime" | "WorkstationDataSystem.RebaseTimeline" | "WorkstationDataSystem.SetClipWarp" | "WorkstationDataSystem.SetWarpMap" | "WorkstationDataSystem.PumpWarp" | "WorkstationDataSystem.AddSpectralMask" | "WorkstationDataSystem.UndoSpectralMask" | "WorkstationDataSystem.ClearSpectralMasks" | "WorkstationDataSystem.ClipEditorPreview" |
            "WorkstationDataSystem.BeginMasterAnalysis" | "WorkstationDataSystem.AnalyzeMaster" | "WorkstationDataSystem.LocatePanel" |
            "WorkstationDataSystem.MixPreview" | "WorkstationDataSystem.StageLighting" | "WorkstationDataSystem.ProjectPanel" => self.workstation.dispatch(target.rsplit('.').next().unwrap(),&input),
            "RayTracedAudioSystem.AdvanceTerrain" => crate::base_system::layer_two::terrain_audio::advance(&mut self.ray_audio,&input),
            "EnvironmentAudioSystem.PreviewAudio" | "EnvironmentAudioSystem.RefreshMix" | "EnvironmentAudioSystem.Begin" | "EnvironmentAudioSystem.Frame" | "EnvironmentAudioSystem.Close" | "EnvironmentAudioSystem.Status" | "EnvironmentAudioSystem.Transport" | "EnvironmentAudioSystem.Pump" | "EnvironmentAudioSystem.Retrospective" | "EnvironmentAudioSystem.DrainCapture" | "EnvironmentAudioSystem.MidiInput" | "EnvironmentAudioSystem.MidiKeyboard" => crate::base_system::layer_five::environment_audio_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "CaveSystem.Configure" | "CaveSystem.Snapshot" | "CaveSystem.Sample" | "CaveSystem.Support" | "CaveSystem.Mesh" | "CaveSystem.ResolveBody" => {
                let root=self.root.clone();
                crate::base_system::layer_two::cave_system::dispatch(&mut self.caves,&root,target.rsplit('.').next().unwrap(),&input)
            },
            "TerrainGeometrySystem.Capture" | "TerrainGeometrySystem.Trace" | "TerrainGeometrySystem.MaterialIntervals" => crate::base_system::layer_two::terrain_geometry_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "JackConnectionSystem.Attach" | "JackConnectionSystem.Info" => crate::base_system::layer_one::jack_connection_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "IOSAudioConnectionSystem.Attach" | "IOSAudioConnectionSystem.Info" => crate::base_system::layer_one::ios_audio_connection_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "ChuckConnectionSystem.AttachOutput" => {
                let id=input["entity"].as_u64().unwrap();
                let backend=input["backend"].as_str().unwrap();
                let native_id=if backend=="ios" { self.ios_audio.prepared_entity() } else { self.audio.prepared_entity() };
                if self.chuck.prepared_entity()!=Some(id) || native_id!=Some(id) {
                    Err("ChucK and native output must own the same prepared sound Entity".into())
                } else {
                    let output=self.chuck.output_slot();
                    if backend=="ios" { self.ios_audio.attach_output(id,output).map(|_|Value::Null) }
                    else { self.audio.attach_output(id,output).map(|_|Value::Null) }
                }
            }
            "ChuckConnectionSystem.Release" => {
                let id=self.chuck.prepared_entity();
                if id.is_some() && (self.audio.prepared_entity()==id || self.ios_audio.prepared_entity()==id) {
                    Err("release native callback before ChucK VM".into())
                } else {
                    let root=self.root.clone();
                    self.chuck.dispatch(&root,"Release",&input)
                }
            }
            "TideSystem.Configure" | "TideSystem.Step" | "TideSystem.Sample" | "TideSystem.Close" => crate::base_system::layer_two::tide_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "OceanWaveSystem.Configure" | "OceanWaveSystem.Step" | "OceanWaveSystem.Sample" | "OceanWaveSystem.Snapshot" | "OceanWaveSystem.Close" => crate::base_system::layer_two::ocean_wave_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "WaterFlowSystem.Configure" | "WaterFlowSystem.Step" | "WaterFlowSystem.Sample" | "WaterFlowSystem.Close" => crate::base_system::layer_two::water_flow_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "WaterSlideSystem.Configure" | "WaterSlideSystem.Edit" | "WaterSlideSystem.Course" | "WaterSlideSystem.CollisionMesh" | "WaterSlideSystem.Snapshot" | "WaterSlideSystem.Close" => crate::base_system::layer_two::water_slide_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "FloodGateSystem.Configure" | "FloodGateSystem.Step" | "FloodGateSystem.Close" => crate::base_system::layer_two::flood_gate_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "SoftBodyWaterSystem.Configure" | "SoftBodyWaterSystem.Step" | "SoftBodyWaterSystem.Gates" | "SoftBodyWaterSystem.Sample" | "SoftBodyWaterSystem.Disturb" | "SoftBodyWaterSystem.Snapshot" | "SoftBodyWaterSystem.Close" => crate::base_system::layer_two::soft_body_water_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "TerrainSurfaceHydrologySystem.AudioSources" | "TerrainSurfaceHydrologySystem.MaterialPreview" | "TerrainSurfaceHydrologySystem.MaterialDomain" | "TerrainSurfaceHydrologySystem.Drainage" | "TerrainSurfaceHydrologySystem.Configure" | "TerrainSurfaceHydrologySystem.Update" | "TerrainSurfaceHydrologySystem.Raycast" | "TerrainSurfaceHydrologySystem.Sample" | "TerrainSurfaceHydrologySystem.SurfaceMesh" | "TerrainSurfaceHydrologySystem.Revision" | "TerrainSurfaceHydrologySystem.Snapshot" | "TerrainSurfaceHydrologySystem.Close" => crate::base_system::layer_two::terrain_surface_hydrology_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "WaterRenderSystem.Prepare" | "WaterRenderSystem.Update" | "WaterRenderSystem.Frame" | "WaterRenderSystem.Close" => crate::base_system::layer_four::water_render_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "WaterSystem.Configure" | "WaterSystem.Step" | "WaterSystem.Frame" | "WaterSystem.Sample" | "WaterSystem.Disturb" | "WaterSystem.Edit" | "WaterSystem.Pause" | "WaterSystem.Close" => crate::base_system::layer_five::water_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "VegetationSystem.MaterialTree" | "VegetationSystem.QueryWood" | "VegetationSystem.QueryPerches" | "VegetationSystem.QueryPlants" | "VegetationSystem.QueryGeometry" | "VegetationSystem.AdvanceGrowth" | "VegetationSystem.ApplyDamage" | "VegetationSystem.Snapshot" | "VegetationSystem.Restore" => crate::base_system::layer_two::vegetation_system::dispatch(&mut self.vegetation, target.rsplit('.').next().unwrap(), &input),
            "ChuckConnectionSystem.SetMicrophoneRoutes" | "ChuckConnectionSystem.RefreshMix" | "ChuckConnectionSystem.Prepare" | "ChuckConnectionSystem.Compile" | "ChuckConnectionSystem.Remove" | "ChuckConnectionSystem.Poll" | "ChuckConnectionSystem.SetRoutes" | "ChuckConnectionSystem.Telemetry" => { let root=self.root.clone(); self.chuck.dispatch(&root,target.rsplit('.').next().unwrap(),&input) },
            "RayTracedAudioSystem.Advance" | "RayTracedAudioSystem.Snapshot" | "RayTracedAudioSystem.Invalidate" | "RayTracedAudioSystem.AdvanceMicrophone" => self.ray_audio.dispatch(target.rsplit('.').next().unwrap(),&input),
            "SoundPhysicsSystem.Apply" | "SoundPhysicsSystem.ApplyMicrophone" => crate::base_system::layer_two::sound_physics_system::dispatch(target.rsplit('.').next().unwrap(),&input),
            "MonasterySystem.Input" | "MonasterySystem.Snapshot" | "MonasterySystem.Close" => { let mut monastery=std::mem::take(&mut self.monastery); let result=monastery.dispatch(self,target.rsplit('.').next().unwrap(),&input); self.monastery=monastery; result },
            "MonasteryRenderSystem.Frame" => crate::base_system::layer_four::monastery_render_system::dispatch(self,"Frame",&input),
            "LaboratorySystem.Step" | "LaboratorySystem.Snapshot" | "LaboratorySystem.Close" => { let mut laboratory=std::mem::take(&mut self.laboratory); let result=laboratory.dispatch(self,target.rsplit('.').next().unwrap(),&input); self.laboratory=laboratory; result },
            "NpcSystem.Step" | "NpcSystem.Snapshot" | "NpcSystem.Close" => { let mut npcs=std::mem::take(&mut self.npcs); let result=npcs.dispatch(self,target.rsplit('.').next().unwrap(),&input); self.npcs=npcs; result },
            "SilverSpiderCrabSystem.Step" | "SilverSpiderCrabSystem.Close" => { let mut crab=std::mem::take(&mut self.silver_spider_crab); let result=crab.dispatch(self,target.rsplit('.').next().unwrap(),&input); self.silver_spider_crab=crab; result },
            "DeerSystem.Step" | "DeerSystem.Snapshot" | "DeerSystem.Close" => { let mut deer=std::mem::take(&mut self.deer); let result=deer.dispatch(self,target.rsplit('.').next().unwrap(),&input); self.deer=deer; result },
            "BirdSystem.Step" | "BirdSystem.Snapshot" | "BirdSystem.Close" => { let mut birds=std::mem::take(&mut self.birds); let result=birds.dispatch(self,target.rsplit('.').next().unwrap(),&input); self.birds=birds; result },
            "RegistrySystem.State" | "RegistrySystem.SetEnabled" => crate::base_system::layer_two::registry_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "AudicleSystem.Fail" | "AudicleSystem.Resume" => crate::base_system::layer_five::audicle_system::dispatch(self, target.rsplit('.').next().unwrap(), &input),
            "PlanetNavigationSystem.Abort" => crate::base_system::layer_five::environment_system::dispatch(self,"Abort",&input),
            "FlightSystem.Abort" => crate::base_system::layer_five::flight_system::dispatch(self,"Abort",&input),
            "ShovelSystem.Abort" => crate::base_system::layer_five::shovel_system::dispatch(self,"Abort",&input),
            "NativeInventorySystem.Probe" => {
                crate::base_system::layer_one::native_inventory_system::probe()
            }
            "DiagnosticsSystem.ValidateRuntime" => {
                crate::base_system::layer_two::diagnostics_system::validate_runtime(self)
            }
            "DiagnosticsSystem.Report" => {
                crate::base_system::layer_two::diagnostics_system::report(self)
            }
            "DiagnosticsSystem.CheckAudio" => {
                crate::base_system::layer_two::diagnostics_system::check_audio(self)
            }
            "JackConnectionSystem.CheckConnection" => {
                crate::base_system::layer_one::jack_connection_system::check_connection()
            }
            "JackConnectionSystem.Prepare"
            | "JackConnectionSystem.Start"
            | "JackConnectionSystem.Stop"
            | "JackConnectionSystem.Release" => {
                crate::base_system::layer_one::jack_connection_system::dispatch(
                    self,
                    target.split_once('.').unwrap().1,
                    &input,
                )
            }
            "IOSAudioConnectionSystem.Complete" |
            "IOSAudioConnectionSystem.RouteChanged" |
            "IOSAudioConnectionSystem.Prepare"
            | "IOSAudioConnectionSystem.Start"
            | "IOSAudioConnectionSystem.Stop"
            | "IOSAudioConnectionSystem.Release" => {
                crate::base_system::layer_one::ios_audio_connection_system::dispatch(
                    self,
                    target.split_once('.').unwrap().1,
                    &input,
                )
            }
            "EntitySystem.Create" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Create", &input)
            }
            "EntitySystem.Update" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Update", &input)
            }
            "GraphicsConnectionSystem.PluginSurface"=>crate::base_system::layer_one::graphics_connection_system::dispatch(self,"PluginSurface",&input),
            "GraphicsConnectionSystem.CcParameterPicker"=>crate::base_system::layer_one::graphics_connection_system::dispatch(self,"CcParameterPicker",&input),
            "GraphicsConnectionSystem.SelectionRange"=>crate::base_system::layer_one::graphics_connection_system::dispatch(self,"SelectionRange",&input),
            "GraphicsConnectionSystem.CursorLabel" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(self,"CursorLabel",&input)
            }
            "GraphicsConnectionSystem.Caption" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Caption", &input,
                )
            }
            "WorkshopSystem.Run" => {
                crate::base_system::layer_five::workshop_system::run(self, &input)
            }
            "EntitySystem.Get" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Get", &input)
            }
            "EntitySystem.Remove" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Remove", &input)
            }
            "AudicleSystem.Create" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Create", &input)
            }
            "AudicleSystem.Ready" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Ready", &input)
            }
            "AudicleSystem.Go" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Go", &input)
            }
            "AudicleSystem.Finish" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Finish", &input)
            }
            "AudicleSystem.Cancel" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Cancel", &input)
            }
            "AppearanceSystem.Prepare" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Prepare", &input)
            }
            "AppearanceSystem.Present" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Present", &input)
            }
            "AppearanceSystem.Release" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Release", &input)
            }
            "GraphicsConnectionSystem.Prepare" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Prepare", &input,
                )
            }
            "GraphicsConnectionSystem.Present" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Present", &input,
                )
            }
            "GraphicsConnectionSystem.Release" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Release", &input,
                )
            }
            "CueProcedureSystem.Load" => {
                crate::base_system::layer_two::cue_procedure_system::load(self, &input)
            }
            "PlanetNavigationSystem.Step" | "PlanetNavigationSystem.Stop" | "PlanetNavigationSystem.Configure" | "PlanetNavigationSystem.Pause" => {
                crate::base_system::layer_five::environment_system::dispatch(
                    self,
                    target.rsplit('.').next().unwrap(),
                    &input,
                )
            }
            "ShovelSystem.Step" | "ShovelSystem.Active" | "ShovelSystem.Close" | "ShovelSystem.ConstrainMovement" =>
                crate::base_system::layer_five::shovel_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "ShovelSoilSystem.Configure" | "ShovelSoilSystem.Step" | "ShovelSoilSystem.Mesh" | "ShovelSoilSystem.Report" | "ShovelSoilSystem.Release" =>
                crate::base_system::layer_two::shovel_soil_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "PlanetSurfaceSystem.CollisionSnapshot" | "PlanetSurfaceSystem.Frame"
            | "PlanetSurfaceSystem.SuspendMaterial"
            | "PlanetSurfaceSystem.Height"
            | "PlanetSurfaceSystem.Excavate"
            | "PlanetSurfaceSystem.Advance"
            | "PlanetSurfaceSystem.Save" => {
                crate::base_system::layer_two::planet_surface_system::dispatch(
                    self,
                    target.rsplit('.').next().unwrap(),
                    &input,
                )
            }
            "PhysicsSystem.GameCraftTree" | "PhysicsSystem.GameWaterSample" | "PhysicsSystem.GameWater" | "PhysicsSystem.GameResolveBody" | "PhysicsSystem.GameCollision" | "PhysicsSystem.GameStep" | "PhysicsSystem.GameFrame" | "PhysicsSystem.GameHeight" | "PhysicsSystem.GamePause" | "PhysicsSystem.GameClose" | "PhysicsSystem.Configure"
            | "PhysicsSystem.Advance"
            | "PhysicsSystem.Report"
            | "PhysicsSystem.Mesh" => crate::base_system::layer_two::physics_system::dispatch(
                self,
                target.rsplit('.').next().unwrap(),
                &input,
            ),
            "WorkshopSystem.Benchmark" => {
                crate::base_system::layer_five::workshop_system::benchmark(self, &input)
            }
            "PhysicsSystem.Release" => crate::physics::dispatch(self, "Release", &input),
            "OGCConnectionSystem.Ownership" => Ok(self.ogc.ownership(self.invoking_system())),
            "OGCConnectionSystem.Release" => {
                let owner = self.invoking_system().to_owned();
                self.ogc.release_for(&owner)
            }
            "OGCConnectionSystem.Synchronize" => {
                self.ogc.assert_owner(self.invoking_system())
                    .and_then(|_| self.ogc.synchronize(&input))
            }
            "OGCConnectionSystem.Configure" => {
                let owner = self.invoking_system().to_owned();
                self.ogc.configure_for(&owner, &input)
            }
            "OGCConnectionSystem.Project" => {
                self.ogc.assert_owner(self.invoking_system())
                    .and_then(|_| self.ogc.project(&input))
            },
            "OGCConnectionSystem.Respond" => {
                self.ogc.assert_owner(self.invoking_system())
                    .and_then(|_| self.ogc.respond_json(&input))
            },
            "FlightSystem.Step" | "FlightSystem.Stop" => {
                crate::base_system::layer_five::flight_system::dispatch(
                    self,
                    target.rsplit('.').next().unwrap(),
                    &input,
                )
            }
            "CloudSystem.Frame" => crate::base_system::layer_four::cloud_system::dispatch(&input),
            "AppearanceSystem.View" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "View", &input)
            }
            "GraphicsConnectionSystem.InputMode" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(self,"InputMode",&input)
            }
            "GraphicsConnectionSystem.SetView" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "SetView", &input,
                )
            }
            "PlayerModelSystem.Load"=>crate::base_system::layer_two::player_model_system::load(self,&input),
            "InverseKinematicsSystem.Configure"|"InverseKinematicsSystem.SolveChains"|"InverseKinematicsSystem.Solve"|"InverseKinematicsSystem.Active"|"InverseKinematicsSystem.Release"=>crate::base_system::layer_two::inverse_kinematics_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "PlayerRigSystem.Prepare"|"PlayerRigSystem.Plan"|"PlayerRigSystem.Advance"|"PlayerRigSystem.Release"=>crate::base_system::layer_four::player_rig_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "GraphicsConnectionSystem.Texture"|"GraphicsConnectionSystem.ReleaseTexture"=>crate::base_system::layer_one::graphics_connection_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "ApplicationSystem.Run" => {
                crate::base_system::layer_five::application_system::run(self, &input)
            }
            "CueSystem.Begin" | "CueSystem.Advance" | "CueSystem.Cancel" | "CueSystem.Close"
            | "CueSystem.Status" => crate::base_system::layer_five::cue_system::dispatch(
                self,
                target.rsplit('.').next().unwrap(),
                &input,
            ),
            "ColorEmotionSystem.Advance"|"ColorEmotionSystem.Active"|"ColorEmotionSystem.Release" => crate::base_system::layer_four::color_emotion_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "TerrainToolSystem.Prepare"|"TerrainToolSystem.Aim"|"TerrainToolSystem.SurfaceHeight"|"TerrainToolSystem.Cut"|"TerrainToolSystem.Advance"|"TerrainToolSystem.Frame"|"TerrainToolSystem.Release" => crate::base_system::layer_two::terrain_tool_system::dispatch(self,target.rsplit('.').next().unwrap(),&input),
            "PlanetSurfaceSystem.Reset" => crate::physics::planet::dispatch(self,"Reset",&input),
            "PlanetSurfaceSystem.ReleaseMaterials" => crate::physics::planet::dispatch(self,"ReleaseMaterials",&input),
            "PlanetSurfaceSystem.RefineMaterial" => crate::physics::planet::dispatch(self,"RefineMaterial",&input),
            "PlanetSurfaceSystem.MineCell" => crate::physics::planet::dispatch(self,"MineCell",&input),
            "PlanetSurfaceSystem.BladeCut" => crate::physics::planet::dispatch(self,"BladeCut",&input),
            "AppearanceSystem.Pump" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Pump", &input)
            }
            "GraphicsConnectionSystem.Pump" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Pump", &input,
                )
            }
            "JackConnectionSystem.RouteChanged" => {
                crate::base_system::layer_one::jack_connection_system::dispatch(
                    self,
                    "RouteChanged",
                    &input,
                )
            }
            "JackConnectionSystem.Complete" => {
                crate::base_system::layer_one::jack_connection_system::dispatch(
                    self, "Complete", &input,
                )
            }
            "CueSystem.Run" => crate::base_system::layer_five::cue_system::run(self, &input),
            _ => Err(format!("no implementation for {target}")),
        }}
        .and_then(|value| {
            if check_payload(&hook.output, &value) {
                Ok(value)
            } else {
                Err(format!("invalid output for {target}"))
            }
        });
        self.stack.pop();
        let elapsed_us=started.elapsed().as_micros().min(u64::MAX as u128)as u64;
        let slow_cpu_us=if elapsed_us>=5000{_performance.inclusive_cpu_ns().map(|n|n/1000)}else{None};
        let mut completion=json!({"event":if result.is_ok(){"finished"}else{"failed"},
            "invocation":id,"parent":parent,"function":target,"layer":hook.layer,
            "output_type":hook.output,"audicle":event_audicle.or(self.active_audicle),"elapsed_us":elapsed_us,"error":result.as_ref().err()});
        if let Some(cpu_us)=slow_cpu_us{completion["slow_call_inclusive_thread_cpu_us"]=json!(cpu_us);}
        self.record(completion)?;
        drop(_performance);
        if target == "GraphicsConnectionSystem.Pump" && self.performance.report_due() {
            self.write_performance_report()?;
        }
        result
    }

    pub(crate) fn system_enabled(&self, system:&str)->bool { self.enabled.get(system).copied().unwrap_or(false) }
    pub(crate) fn registry_state(&self) -> Value {
        json!({"systems":self.enabled,"initialized":self.initialized,"initialization_failed":self.initialization_failed})
    }
    fn system_uses(&self, owner: &str, target: &str) -> bool {
        let mut pending = vec![owner.to_owned()];
        let mut visited = BTreeSet::new();
        while let Some(system) = pending.pop() {
            if system == target { return true; }
            if !visited.insert(system.clone()) { continue; }
            for hook in self.hooks.values().filter(|hook| hook.system == system) {
                for call in &hook.calls { pending.push(self.hooks[call].system.clone()); }
            }
        }
        false
    }
    pub(crate) fn set_system_enabled(&mut self, system: &str, enabled: bool) -> Result<Value,String> {
        let old = *self.enabled.get(system).ok_or("unknown System")?;
        if old == enabled { return Ok(self.registry_state()); }
        if !enabled {
            if self.stack.iter().any(|(_, hook)| self.hooks[hook].system == system) {
                return Err("cannot disable a System on the invocation stack".into());
            }
            for owner in self.enabled.keys() {
                let resources = self.entities.owned_by(owner) || self.audicles.owned_by(owner)
                    || (owner == "WorkstationDataSystem" && self.workstation.export_active())
                    || (owner == "GraphicsConnectionSystem" && self.graphics.prepared_entity().is_some())
                    || (owner == "JackConnectionSystem" && self.audio.prepared_entity().is_some())
                    || (owner == "IOSAudioConnectionSystem" && self.ios_audio.prepared_entity().is_some())
                    || (owner == "ChuckConnectionSystem" && self.chuck.prepared_entity().is_some())
                    || self.ogc.ownership(owner)["owned_by_caller"] == true;
                if resources && self.system_uses(owner,system) {
                    return Err(format!("{system} supports live events/resources owned by {owner}; close/release them before disabling"));
                }
            }
        } else {
            if self.initialization_failed.contains(system) {
                return Err("System initialization previously failed; restart for explicit recovery".into());
            }
            for hook in self.all_init.get(system).into_iter().flatten() {
                if !self.hooks["RegistrySystem.SetEnabled"].calls.contains(hook) {
                    return Err(format!("runtime activation init route is undeclared: {hook}"));
                }
                for target in &self.hooks[hook].calls {
                    let dependency = &self.hooks[target].system;
                    if dependency != system && !self.enabled[dependency] {
                        return Err(format!("initialization {hook} requires disabled System {dependency}"));
                    }
                }
            }
        }
        self.enabled.insert(system.into(), enabled);
        if enabled && !self.initialized.contains(system) {
            for hook in self.all_init.get(system).cloned().unwrap_or_default() {
                if let Err(error) = self.invoke(&hook, Value::Null) {
                    self.enabled.insert(system.into(), false);
                    self.initialization_failed.insert(system.into());
                    self.record(json!({"event":"system_activation","system":system,"enabled":false,"outcome":"initialization_failed","error":error}))?;
                    return Err(error);
                }
            }
            self.initialized.insert(system.into());
        }
        self.record(json!({"event":"system_activation","system":system,"enabled":enabled,"outcome":"applied"}))?;
        Ok(self.registry_state())
    }

    pub(crate) fn startup(&mut self) -> Result<Value, String> {
        if self.startup_attempted {
            return Err("Host startup has already been attempted".into());
        }
        self.startup_attempted = true;
        self.record(
            json!({"event":"host_startup","state":"Starting","systems":self.startup_order}),
        )?;
        let mut steps = Vec::new();
        let mut failure = None;
        for hook in self.startup_steps.clone() {
            let system = self.hooks[&hook].system.clone();
            if self.initialized.contains(&system) { continue; }
            match self.invoke(&hook, Value::Null) {
                Ok(output) => {
                    steps.push(json!({"function":hook,"output":output}));
                    if self.all_init.get(&system).and_then(|hooks| hooks.last()) == Some(&hook) {
                        self.initialized.insert(system);
                    }
                }
                Err(error) => {
                    self.initialization_failed.insert(system);
                    failure = Some((hook, error));
                    break;
                }
            }
        }
        if failure.is_none() {
            self.initialized.extend(self.startup_order.iter().cloned());
        }
        let report = json!({"mode":"system_startup","started":failure.is_none(),"systems_order":self.startup_order,"completed_steps":steps,"failure":failure});
        self.record(json!({"event":"host_startup","state":if failure.is_none(){"Started"}else{"Failed"},"failure":failure}))?;
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        fs::write(
            self.output.join("startup.json"),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        match failure {
            Some((hook, error)) => Err(format!("startup failed at {hook}: {error}")),
            None => Ok(report),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os="macos")]
    #[test]
    fn ambient_session_reuses_requests_and_releases_entities() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory=std::env::temp_dir().join(format!("wormwood-ambient-lifecycle-{}",std::process::id()));
        let mut host=Host::load_paths(root,&directory).unwrap();
        host.stack.push((1200,"EnvironmentAudioSystem.Begin".into()));
        let event=host.invoke("AudicleSystem.Create",Value::Null).unwrap();
        let sound=host.invoke("EntitySystem.Create",json!({"kind":"sound","source":"embedded_chuck"})).unwrap();
        host.invoke("ChuckConnectionSystem.Prepare",json!({"entity":sound["entity"],"sample_rate":48000})).unwrap();
        host.invoke("AudicleSystem.Ready",event.clone()).unwrap();
        host.invoke("AudicleSystem.Go",event.clone()).unwrap();
        host.stack.clear();host.stack.push((1201,"EnvironmentAudioSystem.Frame".into()));
        let mut state=crate::base_system::layer_five::environment_audio_system::ambience::State::default();
        state.step(&mut host,[0.,6371002.,0.],[0.,1.,0.],0.).unwrap();
        let requests=state.snapshot();
        let entities=requests["entities"].as_array().unwrap().clone();
        state.step(&mut host,[0.,6371002.,0.],[0.,1.,0.],0.3).unwrap();
        assert_eq!(state.snapshot()["requests"],requests["requests"]);assert_eq!(state.snapshot()["entities"],requests["entities"]);
        host.stack.clear();host.stack.push((1202,"EnvironmentAudioSystem.Close".into()));
        state.close(&mut host).unwrap();state.close(&mut host).unwrap();
        for id in entities {assert!(host.entities.snapshot(id.as_u64().unwrap()).is_err());}
        host.invoke("ChuckConnectionSystem.Release",json!({"output_detached":true})).unwrap();
        host.invoke("EntitySystem.Remove",sound).unwrap();
        host.invoke("AudicleSystem.Finish",event).unwrap();
        drop(host);let _=std::fs::remove_dir_all(directory);
    }

    #[test]
    fn npc_intent_contract_and_source_bindings() {
        let report=crate::host::intent::inspect(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        assert_eq!(report["summary"]["active_structural_mismatches"],0);
        assert!(report["behaviors"].as_array().unwrap().iter().any(|b|b["id"]=="NpcSystem.waterpark_placeholder_visitors"));
    }

    #[cfg(unix)]
    #[test]
    fn registered_npc_waterpark_routines_dialogue_and_cleanup() {
        use crate::audio::{add,sub,length,vector};
        // Load the real NPC, Entity, Audicle and Theatre contracts in isolation:
        // unrelated application Systems need not be complete to test this boundary.
        let source=Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture=std::env::temp_dir().join(format!("wormwood-npc-contracts-{}",std::process::id()));
        fs::create_dir_all(fixture.join("BaseSystem")).unwrap();
        fs::create_dir_all(fixture.join("Systems")).unwrap();
        fs::create_dir_all(fixture.join("Procedures")).unwrap();
        fs::create_dir_all(fixture.join("output")).unwrap();
        let names=["NpcSystem","EntitySystem","AudicleSystem","TheatreSystem","PlanetNavigationSystem"];
        let mut registry=serde_json::Map::new();
        let mut catalog=read(source,"Procedures/functionregistry.json").unwrap();
        catalog["systems"].as_object_mut().unwrap().retain(|name,_|names.contains(&name.as_str()));
        for name in names {
            registry.insert(name.into(),json!(true));
            let mut contract=read(source,&format!("Systems/{name}.json")).unwrap();
            if name=="PlanetNavigationSystem" {
                catalog["systems"][name]["functions"].as_object_mut().unwrap().retain(|hook,_|["Step","Stop"].contains(&hook.as_str()));
                contract=json!({"system":name,"layer":5,"functions":{"Step":{"calls":["NpcSystem.Step"]},"Stop":{"calls":["NpcSystem.Close"]}},"init_steps":[]});
            }
            fs::write(fixture.join(format!("Systems/{name}.json")),serde_json::to_vec(&contract).unwrap()).unwrap();
        }
        fs::write(fixture.join("BaseSystem/registry.json"),serde_json::to_vec(&registry).unwrap()).unwrap();
        fs::write(fixture.join("BaseSystem/circuit_breaker.json"),br#"{"halt_execution":false}"#).unwrap();
        fs::write(fixture.join("Procedures/functionregistry.json"),serde_json::to_vec(&catalog).unwrap()).unwrap();
        fs::write(fixture.join("Procedures/systems.json"),serde_json::to_vec(&json!({"systems_order":names.map(|n|format!("{n}.json"))})).unwrap()).unwrap();
        for entry in fs::read_dir(source.join("Procedures")).unwrap() {
            let entry=entry.unwrap();let name=entry.file_name();
            if name!="functionregistry.json" && name!="systems.json" {
                std::os::unix::fs::symlink(entry.path(),fixture.join("Procedures").join(name)).unwrap();
            }
        }
        let mut h=Host::load_paths(&fixture,&fixture.join("output")).unwrap();
        h.graphics.test_driver=true;
        h.invoke("TheatreSystem.Configure",Value::Null).unwrap();
        let entities_before=h.entities.len();
        let audicles_before=h.audicles.count();
        h.stack.push((1400,"PlanetNavigationSystem.Step".into()));
        let mut request=json!({"position":h.theatre.origin,"forward":[0.,0.,-1.],"dt":0.,"talk":false});
        let initial=h.invoke("NpcSystem.Step",request.clone()).unwrap();
        assert_eq!(initial["visitors"].as_array().unwrap().len(),3);
        assert_eq!(h.entities.len(),entities_before+3);
        assert!(h.active_audicle.is_none());
        let ids:Vec<_>=initial["instances"].as_array().unwrap().iter().map(|i|i["entity"].as_u64().unwrap()).collect();
        for id in ids {let e=h.entities.snapshot(id).unwrap().data();assert_eq!(e["shader"],"unlit_surface");assert_eq!(e["vertices"].as_array().unwrap().len(),672);}
        let start=vector(&initial["visitors"][0]["position"]).unwrap();
        request["position"]=json!(add(start,[0.,0.,2.]));
        let near=h.invoke("NpcSystem.Step",request.clone()).unwrap();
        assert!(near["caption"].as_str().unwrap().contains("T talk"));
        request["talk"]=json!(true);
        let first=h.invoke("NpcSystem.Step",request.clone()).unwrap();
        assert!(first["caption"].as_str().unwrap().contains("One more lap"));
        assert_eq!(h.invoke("NpcSystem.Step",request.clone()).unwrap()["caption"],first["caption"]);
        request["talk"]=json!(false);h.invoke("NpcSystem.Step",request.clone()).unwrap();
        request["talk"]=json!(true);
        assert!(h.invoke("NpcSystem.Step",request.clone()).unwrap()["caption"].as_str().unwrap().contains("practicing"));
        // Invalid deltas fail without moving anyone or consuming a dialogue edge.
        request["dt"]=json!(1.);
        assert!(h.invoke("NpcSystem.Step",request.clone()).is_err());
        request["dt"]=json!(0.25);request["talk"]=json!(false);
        let moved=h.invoke("NpcSystem.Step",request.clone()).unwrap();
        assert!((length(sub(vector(&moved["visitors"][0]["position"]).unwrap(),start))-0.375).abs()<1e-7);
        request["dt"]=json!(0.);
        let paused=h.invoke("NpcSystem.Step",request.clone()).unwrap();
        assert_eq!(paused["visitors"],moved["visitors"]);
        request["position"]=json!(h.theatre.origin);
        assert!(h.invoke("NpcSystem.Step",request).unwrap()["caption"].is_null());
        h.stack.clear();h.stack.push((1401,"PlanetNavigationSystem.Stop".into()));
        h.invoke("NpcSystem.Close",Value::Null).unwrap();h.invoke("NpcSystem.Close",Value::Null).unwrap();
        assert_eq!(h.entities.len(),entities_before);assert_eq!(h.audicles.count(),audicles_before);assert!(h.active_audicle.is_none());
        drop(h);fs::remove_dir_all(fixture).unwrap();
    }

    #[test]
    fn silver_spider_crab_registered_depths_encounter_and_cleanup() {
        let mut h=traced_host();
        h.enabled.insert("WaterSystem".into(),false);
        h.invoke("CaveSystem.Configure",Value::Null).unwrap();
        let entities=h.entities.len();let events=h.audicles.count();
        h.stack.push((1450,"PlanetNavigationSystem.Step".into()));
        let request=|p:crate::audio::V,keys:[bool;4]|json!({"position":p,"forward":[0.,0.,1.],"up":[0.,1.,0.],"dt":0.1,"move":keys});
        let center=crate::terrain::underground::world([-51.,-44.,16.]);
        let mut result=h.invoke("SilverSpiderCrabSystem.Step",request(center,[false;4])).unwrap();
        assert_eq!(result["instances"].as_array().unwrap().len(),1,"{result}");
        let start=crate::audio::vector(&result["position"]).unwrap();
        for _ in 0..10 {result=h.invoke("SilverSpiderCrabSystem.Step",request(center,[false;4])).unwrap();}
        let moved=crate::audio::vector(&result["position"]).unwrap();
        assert!(crate::audio::length(crate::audio::sub(moved,start))>0.1,"crab must move through real cave air");
        let observer=crate::audio::add(moved,[0.,1.,0.]);
        result=h.invoke("SilverSpiderCrabSystem.Step",request(observer,[false;4])).unwrap();assert_eq!(result["locked"],true);
        for _ in 0..24 {h.invoke("SilverSpiderCrabSystem.Step",request(observer,[false;4])).unwrap();result=h.invoke("SilverSpiderCrabSystem.Step",request(observer,[true,false,false,false])).unwrap();}
        assert_eq!(result["locked"],false);assert_eq!(result["mode"],"Retreating");
        assert!(h.active_audicle.is_none());
        h.stack.clear();h.stack.push((1451,"PlanetNavigationSystem.Stop".into()));
        h.invoke("SilverSpiderCrabSystem.Close",Value::Null).unwrap();h.invoke("SilverSpiderCrabSystem.Close",Value::Null).unwrap();
        assert_eq!(h.entities.len(),entities);assert_eq!(h.audicles.count(),events);
        std::fs::remove_file(&h.trace).unwrap();
    }

    #[test]
    fn registered_deer_system_spawns_flees_and_cleans_up_without_audio() {
        let mut h=traced_host();
        for name in ["EnvironmentAudioSystem","WaterSystem","VegetationSystem","CaveSystem"] {h.enabled.insert(name.into(),false);}
        h.stack.push((1300,"PlanetNavigationSystem.Step".into()));
        let initial=h.invoke("DeerSystem.Step",json!({"position":[0.,6371002.,-160.],"dt":0.})).unwrap();
        assert_eq!(initial["animals"].as_array().unwrap().len(),2);
        assert!(h.active_audicle.is_none());
        let start=crate::audio::vector(&initial["animals"][0]["position"]).unwrap();
        let observer=crate::audio::add(start,crate::audio::mul(crate::audio::unit(start),1.8));
        let state=h.invoke("DeerSystem.Step",json!({"position":observer,"dt":0.1})).unwrap();
        assert_eq!(state["animals"][0]["fleeing"],true);
        let mut running=state.clone();
        for _ in 0..20 {running=h.invoke("DeerSystem.Step",json!({"position":observer,"dt":0.1})).unwrap();}
        let escaped=crate::audio::vector(&running["animals"][0]["position"]).unwrap();
        assert!(crate::audio::length(crate::audio::sub(escaped,start))>2.,"deer must actually move on accepted terrain: {running}");
        let id=state["animals"][0]["entity"].as_u64().unwrap();
        assert_eq!(h.entities.snapshot(id).unwrap().data()["vertices"].as_array().unwrap().len(),3686*3);
        assert!(!state["instances"][0]["deer_pose"].is_null());
        assert!(h.invoke("DeerSystem.Step",json!({"position":observer,"dt":2.})).is_err());
        h.stack.clear();h.stack.push((1301,"PlanetNavigationSystem.Stop".into()));
        h.invoke("DeerSystem.Close",Value::Null).unwrap();
        h.invoke("DeerSystem.Close",Value::Null).unwrap();
        assert_eq!(h.entities.len(),0);assert!(h.active_audicle.is_none());
        assert_eq!(h.deer.snapshot()["animals"],json!([]));
        fs::remove_file(&h.trace).unwrap();
    }
    #[test]
    fn registered_deer_ik_tracks_player_preserves_player_state_and_exports_runtime_poses(){
        use crate::audio::{add,mul,vector,unit,length,sub};
        let mut h=traced_host();
        for name in ["EnvironmentAudioSystem","WaterSystem","VegetationSystem","CaveSystem"] {h.enabled.insert(name.into(),false);}
        h.stack.push((1350,"PlanetNavigationSystem.Step".into()));
        let active_before=h.invoke("InverseKinematicsSystem.Active",Value::Null).unwrap();
        let mut state=h.invoke("DeerSystem.Step",json!({"position":[0.,6371002.,-160.],"dt":0.})).unwrap();
        let preview=std::env::var_os("WORMWOOD_DEER_RUNTIME_PREVIEW_DIR").map(std::path::PathBuf::from);
        if let Some(dir)=&preview{fs::create_dir_all(dir).unwrap();}
        let template:Vec<[f32;6]>=serde_json::from_value(crate::base_system::layer_four::deer_rig::spec()["vertices"].clone()).unwrap();
        let lengths=crate::base_system::layer_four::deer_rig::leg_lengths();
        let mut previous_contacts:[Option<[f64;3]>;4]=[None;4];let mut stable_contacts=0; let mut air_frames=0;
        for frame in 0..40 {
            let p=vector(&state["animals"][0]["position"]).unwrap();
            let forward=vector(&state["instances"][0]["deer_pose"]["forward"]).unwrap();
            let observer=add(add(p,mul(forward,-9.)),mul(unit(p),1.8));
            state=h.invoke("DeerSystem.Step",json!({"position":observer,"dt":0.05})).unwrap();
            let mut instance=state["instances"][0].clone();
            let chains=instance["deer_pose"]["ik"]["chains"].as_array().unwrap();assert_eq!(chains.len(),4);
            if instance["deer_pose"]["ik"]["airborne"] == true {
                air_frames += 1;
                assert!(instance["deer_pose"]["ik"]["planted"].as_array().unwrap().iter().all(|p| p == false));
                assert!(instance["deer_pose"]["ik"]["anchors"].as_array().unwrap().iter().all(|p| p.is_null()));
            }
            for i in 0..4 {
                let r=vector(&chains[i]["joints"][0]).unwrap();let m=vector(&chains[i]["joints"][1]).unwrap();let e=vector(&chains[i]["joints"][2]).unwrap();
                assert!((length(sub(m,r))-lengths[i][0]).abs()<1e-4);
                assert!((length(sub(e,m))-lengths[i][1]).abs()<1e-4);
                let pose=&instance["deer_pose"];
                if pose["ik"]["planted"][i]==true && chains[i]["reachable"]==true {
                    let up=vector(&pose["up"]).unwrap();let fwd=vector(&pose["forward"]).unwrap();let right=crate::audio::unit(crate::audio::cross(up,fwd));
                    let origin=vector(&instance["origin"]).unwrap();let tip=add(origin,add(add(mul(right,e[0]),mul(up,e[1])),mul(fwd,e[2])));
                    let anchor=vector(&pose["ik"]["anchors"][i]).unwrap();
                    assert!(length(sub(tip,anchor))<0.012,"IK contact should reach its planted anchor");
                    if let Some(old)=previous_contacts[i] {
                        let delta=sub(tip,old);let horizontal=sub(delta,mul(up,crate::audio::dot(delta,up)));
                        assert!(length(horizontal)<0.015,"planted hooves must not slide with the body");stable_contacts+=1;
                    }
                    previous_contacts[i]=Some(tip);
                }else{previous_contacts[i]=None;}
            }
            if frame>25{assert!(instance["deer_pose"]["look_yaw"].as_f64().unwrap().abs()<0.01);}
            if let Some(dir)=&preview {
                instance["deer_pose"]["up"]=json!([0.,1.,0.]);instance["deer_pose"]["forward"]=json!([0.,0.,1.]);
                let mut vertices=template.clone();crate::base_system::layer_four::deer_rig::animate(&mut vertices,&instance).unwrap();
                fs::write(dir.join(format!("{frame:02}.json")),serde_json::to_vec(&json!({"vertices":vertices,"pose":instance["deer_pose"],"world_origin":state["animals"][0]["position"]})).unwrap()).unwrap();
            }
        }
        assert!(stable_contacts>0); assert!(air_frames>8);
        let observer=vector(&state["animals"][0]["position"]).unwrap();
        h.enabled.insert("InverseKinematicsSystem".into(),false);
        let fallback=h.invoke("DeerSystem.Step",json!({"position":observer,"dt":0.05})).unwrap();
        assert!(fallback["instances"][0]["deer_pose"]["ik"].is_null());
        h.enabled.insert("InverseKinematicsSystem".into(),true);
        let restored=h.invoke("DeerSystem.Step",json!({"position":observer,"dt":0.05})).unwrap();
        assert_eq!(restored["instances"][0]["deer_pose"]["ik"]["chains"].as_array().unwrap().len(),4);
        assert_eq!(h.invoke("InverseKinematicsSystem.Active",Value::Null).unwrap(),active_before);
        h.stack.clear();h.stack.push((1351,"PlanetNavigationSystem.Stop".into()));h.invoke("DeerSystem.Close",Value::Null).unwrap();assert_eq!(h.entities.len(),0);
        fs::remove_file(&h.trace).unwrap();
    }
    #[test]
    fn view_presentation_accepts_eight_registered_scene_instances_and_cleans_up() {
        let mut h=traced_host();
        h.stack.push((1100,"ApplicationSystem.Run".into()));
        h.invoke("CueSystem.Begin",json!({"procedure":"planet_surface"})).unwrap();
        let scene=h.invoke("CueSystem.Advance",Value::Null).unwrap();
        h.stack.clear();h.stack.push((1101,"EnvironmentAudioSystem.Begin".into()));
        let event=h.invoke("AudicleSystem.Create",Value::Null).unwrap();
        let mut instances=Vec::new();
        for _ in 0..2 {
            let entity=h.invoke("EntitySystem.Create",json!({"kind":"visual","shader":"material_surface","vertices":[[-0.1,0.,0.,1.,0.,0.],[0.1,0.,0.,1.,0.,0.],[0.,0.1,0.,1.,0.,0.]]})).unwrap();
            instances.push(json!({"entity":entity["entity"],"origin":[0.,6371001.,-2.]}));
        }
        let sound=h.invoke("EntitySystem.Create",json!({"kind":"sound","source":"embedded_chuck"})).unwrap();
        h.invoke("ChuckConnectionSystem.Prepare",json!({"entity":sound["entity"],"sample_rate":48000})).unwrap();
        h.invoke("AudicleSystem.Ready",event.clone()).unwrap();h.invoke("AudicleSystem.Go",event.clone()).unwrap();
        h.stack.clear();h.stack.push((1102,"EnvironmentAudioSystem.Frame".into()));
        let birds=h.invoke("BirdSystem.Step",json!({"position":[0.,6371002.,0.],"up":[0.,1.,0.],"dt":0.,"time":0.})).unwrap();
        assert_eq!(birds["instances"].as_array().unwrap().len(),4);
        instances.extend(birds["instances"].as_array().unwrap().iter().cloned());
        h.stack.clear();h.stack.push((1105,"PlanetNavigationSystem.Step".into()));
        let old_flags:Vec<_>=["WaterSystem","VegetationSystem","CaveSystem"].into_iter().map(|n|(n,h.enabled.insert(n.into(),false))).collect();
        let deer=h.invoke("DeerSystem.Step",json!({"position":[0.,6371002.,-160.],"dt":0.})).unwrap();
        for (n,v) in old_flags {if let Some(v)=v{h.enabled.insert(n.into(),v);}}
        assert_eq!(deer["instances"].as_array().unwrap().len(),2);
        instances.extend(deer["instances"].as_array().unwrap().iter().cloned());
        assert_eq!(instances.len(),8);
        h.stack.clear();h.stack.push((1106,"EnvironmentAudioSystem.Frame".into()));
        h.invoke("AppearanceSystem.View",json!({"entity":scene["visual"]["entity"],"position":[0.,6371002.,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"right":[1.,0.,0.],"yaw":0.,"pitch":0.,"aspect":1.5,"instances":instances})).unwrap();
        h.stack.clear();h.stack.push((1107,"PlanetNavigationSystem.Stop".into()));
        h.invoke("DeerSystem.Close",Value::Null).unwrap();
        h.stack.clear();h.stack.push((1103,"EnvironmentAudioSystem.Close".into()));
        h.invoke("BirdSystem.Close",Value::Null).unwrap();
        h.invoke("ChuckConnectionSystem.Release",json!({"output_detached":true})).unwrap();
        h.invoke("EntitySystem.Remove",sound).unwrap();
        for instance in instances.into_iter().take(2) {h.invoke("EntitySystem.Remove",json!({"entity":instance["entity"]})).unwrap();}
        h.invoke("AudicleSystem.Finish",event).unwrap();
        h.stack.clear();h.stack.push((1104,"ApplicationSystem.Run".into()));
        h.invoke("CueSystem.Close",Value::Null).unwrap();assert_eq!(h.entities.len(),0);
        fs::remove_file(&h.trace).unwrap();
    }
    #[test]
    fn view_instance_bound_composes_tools_wildlife_and_three_npcs() {
        let mut view=json!({"entity":1,"position":[0.,6371002.,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"right":[1.,0.,0.],"yaw":0.,"pitch":0.,"aspect":1.5,"instances":[]});
        for id in 1..=crate::base_system::layer_four::appearance_system::MAX_SCENE_INSTANCES {view["instances"].as_array_mut().unwrap().push(json!({"entity":id,"origin":[0.,6371000.,0.]}));}
        assert!(check_payload("ViewRequest",&view));
        view["instances"].as_array_mut().unwrap().push(json!({"entity":12,"origin":[0.,6371000.,0.]}));
        assert!(!check_payload("ViewRequest",&view));
    }
    #[cfg(target_os="macos")]
    #[test]
    fn embedded_vm_compiles_original_bird_and_produces_routed_pcm() {
        use crate::audio::{chuck::{Session,Event},dsp::Route};
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let (mut vm,mut output)=Session::prepare(root,48000).unwrap();
        let mut routes=[Route::default();17];
        routes[12]=Route {active:true,gain:0.3,pan:0.,alpha:1.,..Route::default()};
        output.set_routes(routes);
        let request=vm.compile("Procedures/chuck/birds/cardinal.ck",12).unwrap();
        let deadline=std::time::Instant::now()+std::time::Duration::from_secs(3);
        let mut compiled=false;let mut peak=0f32;
        let mut left=[0f32;512];let mut right=[0f32;512];
        while std::time::Instant::now()<deadline && (!compiled || peak<1e-5) {
            output.render(&mut left,&mut right);
            for sample in left.iter().chain(&right) {assert!(sample.is_finite());peak=peak.max(sample.abs());}
            for event in vm.events() {
                match event {
                    Event::Compiled {request:id,..} if id==request => compiled=true,
                    Event::Failed {request:id,error} if id==request => panic!("original bird compile failed: {error}"),
                    _=>{},
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(compiled,"original bird did not compile");
        assert!(peak>1e-5,"original bird produced no routed PCM");
        assert_eq!(vm.telemetry()["render_errors"],0);
        drop(output);
        vm.close().unwrap();
    }
    #[test]
    fn runtime_registry_initializes_once_and_guards_live_dependencies() {
        let mut h = traced_host();
        h.enabled.insert("PlanetNavigationSystem".into(), false);
        h.startup_steps.retain(|name| name != "PlanetNavigationSystem.Configure");
        h.startup_order.retain(|name| name != "PlanetNavigationSystem");
        h.startup().unwrap();
        assert!(!h.initialized.contains("PlanetNavigationSystem"));
        for enabled in [true, false, true] {
            h.invoke("RegistrySystem.SetEnabled", json!({"system":"PlanetNavigationSystem","enabled":enabled})).unwrap();
        }
        let rows: Vec<Value> = fs::read_to_string(&h.trace).unwrap().lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(rows.iter().filter(|r| r["event"] == "started" && r["function"] == "PlanetNavigationSystem.Configure").count(), 1);
        h.stack.push((950, "ApplicationSystem.Run".into()));
        h.invoke("CueSystem.Begin", json!({"procedure":"planet_surface"})).unwrap();
        h.invoke("CueSystem.Advance", Value::Null).unwrap();
        h.stack.pop();
        assert!(h.invoke("RegistrySystem.SetEnabled", json!({"system":"InverseKinematicsSystem","enabled":false})).unwrap_err().contains("live events/resources"));
        h.stack.push((951, "ApplicationSystem.Run".into()));
        h.invoke("CueSystem.Close", Value::Null).unwrap();
        h.stack.pop();
        h.invoke("RegistrySystem.SetEnabled", json!({"system":"InverseKinematicsSystem","enabled":false})).unwrap();
        assert!(!h.enabled["InverseKinematicsSystem"]);
        assert!(h.initialized.contains("InverseKinematicsSystem"));
        fs::remove_file(&h.trace).unwrap();
    }
    #[test]
    fn failed_flight_records_failure_and_foreign_system_cannot_end_event() {
        let mut h = traced_host();
        h.stack.push((900, "ApplicationSystem.Run".into()));
        let bad = json!({"entity":999,"dt":0.02,"move":[true,false,false,false,false,false],"look":[0,0],"reset":false});
        assert!(h.invoke("FlightSystem.Step", bad).is_err());
        assert!(h.active_audicle.is_none());
        let rows: Vec<Value> = fs::read_to_string(&h.trace).unwrap().lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        let failed = rows.iter().find(|r| r["event"] == "audicle_failed").unwrap();
        assert_eq!(failed["owner"], "FlightSystem");
        assert!(!rows.iter().any(|r| r["event"] == "audicle_stage" && r["stage"] == "Finished"));
        h.stack.clear();
        h.stack.push((901, "FlightSystem.Step".into()));
        let event = h.invoke("AudicleSystem.Create", Value::Null).unwrap();
        h.stack.clear();
        h.stack.push((902, "PlanetNavigationSystem.Step".into()));
        assert!(h.invoke("AudicleSystem.Finish", event.clone()).unwrap_err().contains("another System"));
        h.stack.clear();
        h.stack.push((903, "FlightSystem.Step".into()));
        h.invoke("AudicleSystem.Fail", json!({"audicle":event["audicle"],"reason":"test cleanup"})).unwrap();
        assert!(h.active_audicle.is_none());
        fs::remove_file(&h.trace).unwrap();
    }
    #[test]
    fn ogc_registered_callers_cannot_replace_or_release_foreign_context() {
        let mut h = traced_host();
        let scene = json!({"positions":[[0.,0.,0.],[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]],"surfaces":[{"start":0,"count":4,"triangles":[[0,2,1],[0,1,3],[0,3,2],[1,2,3]],"motion":0,"self_collision":false}],"contact_radius":0.008});
        h.stack.push((800, "PhysicsSystem.Configure".into()));
        h.invoke("OGCConnectionSystem.Configure", scene.clone()).unwrap();
        h.stack.pop();
        h.stack.push((801, "ShovelSoilSystem.Configure".into()));
        assert!(h.invoke("OGCConnectionSystem.Configure", scene.clone()).unwrap_err().contains("owned by PhysicsSystem"));
        assert_eq!(h.stack.len(), 1);
        h.stack.pop();
        h.stack.push((802, "ShovelSoilSystem.Release".into()));
        assert!(h.invoke("OGCConnectionSystem.Release", Value::Null).unwrap_err().contains("owned by PhysicsSystem"));
        assert_eq!(h.ogc.ownership("PhysicsSystem")["owned_by_caller"], true);
        assert_eq!(h.stack.len(), 1);
        h.stack.pop();
        h.stack.push((803, "ShovelSoilSystem.Step".into()));
        assert!(h.invoke("OGCConnectionSystem.Project", json!({"positions":scene["positions"],"previous":scene["positions"],"inverse_mass":[1.,1.,1.,1.],"dt":1./240.})).unwrap_err().contains("owned by PhysicsSystem"));
        assert_eq!(h.stack.len(), 1);
        h.stack.pop();
        h.stack.push((804, "TerrainToolSystem.Advance".into()));
        assert!(h.invoke("OGCConnectionSystem.Synchronize", json!({"surface_start":0,"positions":scene["positions"],"previous":scene["positions"]})).unwrap_err().contains("owned by PhysicsSystem"));
        assert_eq!(h.stack.len(), 1);
        h.stack.pop();
        h.stack.push((805, "WorkshopSystem.Benchmark".into()));
        h.invoke("PhysicsSystem.Release", Value::Null).unwrap();
        h.stack.pop();
        assert_eq!(h.ogc.ownership("PhysicsSystem")["configured"], false);
        fs::remove_file(&h.trace).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn ogc_typed_response_preserves_contacts_and_contracts() {
        let mut h = traced_host();
        let p: Vec<[f32;3]> = vec![[-0.1,0.005,-0.1],[0.1,0.005,-0.1],[0.,0.005,0.1],[-1.,0.,-1.],[1.,0.,-1.],[1.,0.,1.],[-1.,0.,1.]];
        let scene = json!({"positions":p,"contact_radius":0.01,"contact_stiffness":5000.,"surfaces":[
            {"start":0,"count":3,"triangles":[[0,1,2]],"motion":0,"self_collision":false},
            {"start":3,"count":4,"triangles":[[0,2,1],[0,3,2]],"motion":2,"self_collision":false}]});
        h.stack.push((1,"PhysicsSystem.Configure".into()));
        h.invoke("OGCConnectionSystem.Configure",scene.clone()).unwrap();
        h.stack.pop(); h.stack.push((2,"PhysicsSystem.Advance".into()));
        let old = h.invoke("OGCConnectionSystem.Project",json!({"positions":p,"previous":p,"inverse_mass":vec![0.;7],"dt":1./240.,"all_responses":true})).unwrap();
        let expected: Vec<[f32;13]> = serde_json::from_value(old["all_responses"].clone()).unwrap();
        let typed = h.contact_responses(&p,&p).unwrap();
        assert!(typed.contacts > 0);
        assert_eq!(typed.responses,expected);
        assert!(h.contact_responses(&p[..6],&p).is_err());
        h.stack.pop(); h.stack.push((3,"PhysicsSystem.Report".into()));
        assert!(h.contact_responses(&p,&p).err().unwrap().contains("undeclared call"));
        h.hooks.get_mut("ShovelSoilSystem.Step").unwrap().calls.insert("OGCConnectionSystem.Respond".into());
        h.stack.pop(); h.stack.push((4,"ShovelSoilSystem.Step".into()));
        assert!(h.contact_responses(&p,&p).err().unwrap().contains("owned by PhysicsSystem"));
        assert_eq!(h.stack.len(),1);
        fs::remove_file(&h.trace).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn ogc_parallel_cached_queries_match_fresh_serial_contact() {
        let mut h = traced_host();
        let mut positions = Vec::<[f32;3]>::new();
        let mut faces = Vec::<[u32;3]>::new();
        for i in 0..280 {
            let x = (i%20) as f32 * 0.025 - 0.25;
            let z = (i/20) as f32 * 0.025 - 0.175;
            let start = positions.len() as u32;
            positions.extend([[x,0.015,z],[x+0.015,0.015,z],[x,0.015,z+0.015]]);
            faces.push([start,start+1,start+2]);
        }
        let n = positions.len();
        positions.extend([[-1.,0.,-1.],[1.,0.,-1.],[1.,0.,1.],[-1.,0.,1.]]);
        let scene = json!({"positions":positions,"contact_radius":0.01,"contact_stiffness":5000.,"surfaces":[
            {"start":0,"count":n,"triangles":faces,"motion":0,"self_collision":false},
            {"start":n,"count":4,"triangles":[[0,2,1],[0,3,2]],"motion":2,"self_collision":false}]});
        let mut parallel = crate::base_system::layer_one::ogc_connection_system::State::default();
        parallel.configure_for("PhysicsSystem", &scene).unwrap();
        // Move into contact through cached candidates, then beyond the cache's
        // displacement envelope. Fresh serial rebuilds are the reference.
        for y in [0.015,0.009,0.004,0.002,0.09,0.005] {
            let old = positions.clone();
            for p in &mut positions[..n] { p[1] = y; }
            let actual = parallel.respond(&positions,&old).unwrap();
            let mut fresh = scene.clone();
            fresh["parallel_queries"] = json!(false);
            fresh["positions"] = json!(positions);
            h.stack.push((1,"PhysicsSystem.Configure".into()));
            h.invoke("OGCConnectionSystem.Configure",fresh).unwrap();
            h.stack.pop(); h.stack.push((2,"PhysicsSystem.Advance".into()));
            let expected = h.contact_responses(&positions,&old).unwrap();
            h.stack.pop();
            assert_eq!(actual.contacts,expected.contacts);
            if y < 0.01 { assert!(actual.contacts > 0); }
            for (a,b) in actual.responses.iter().flatten().zip(expected.responses.iter().flatten()) {
                assert!((a-b).abs() <= 0.0001*(1.+b.abs()), "parallel {a} differs from serial {b}");
            }
        }
        fs::remove_file(&h.trace).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn ogc_parallel_streamed_self_contact_matches_serial() {
        use crate::base_system::layer_one::ogc_connection_system::State;
        let mut p = Vec::<[f32;3]>::new();
        let mut faces = Vec::<[u32;3]>::new();
        for i in 0..70 {
            let x = (i % 10) as f32 * 0.045;
            let z = (i / 10) as f32 * 0.045;
            for y in [0.02, 0.025] {
                let start = p.len() as u32;
                p.extend([[x,y,z],[x+0.025,y,z],[x,y,z+0.025]]);
                faces.push([start,start+1,start+2]);
            }
        }
        let mut scene = json!({"positions":p,"contact_radius":0.01,"contact_stiffness":5000.,"surfaces":[
            {"start":0,"count":p.len(),"triangles":faces,"motion":0,"self_collision":true}]});
        let mut parallel = State::default();
        parallel.configure_for("PhysicsSystem", &scene).unwrap();
        scene["parallel_queries"] = json!(false);
        let mut serial = State::default();
        serial.configure_for("PhysicsSystem", &scene).unwrap();
        for gap in [0.005,0.003,0.02,0.007] {
            let old = p.clone();
            for (i, v) in p.iter_mut().enumerate() { if i % 6 >= 3 { v[1] = 0.02 + gap; } }
            let a = parallel.respond(&p,&old).unwrap();
            let b = serial.respond(&p,&old).unwrap();
            assert_eq!(a.contacts,b.contacts);
            if gap < 0.01 { assert!(a.contacts > 0); }
            for (a,b) in a.responses.iter().flatten().zip(b.responses.iter().flatten()) {
                assert!((a-b).abs() <= 0.0001*(1.+b.abs()), "streamed parallel {a} differs from serial {b}");
            }
        }
    }
    #[test]
    fn player_preparation_precedes_ready_and_scene_close_releases_body() {
        let mut h = traced_host();
        h.stack.push((700, "ApplicationSystem.Run".into()));
        h.invoke("CueSystem.Begin", json!({"procedure":"planet_surface"})).unwrap();
        let event = h.active_audicle.unwrap();
        h.invoke("CueSystem.Advance", Value::Null).unwrap();
        // A finite cue can finish while its scene and prepared player persist.
        assert!(h.active_audicle.is_none());
        assert!(h.entities.len() > 1);
        h.invoke("CueSystem.Close", Value::Null).unwrap();
        h.trace_writer.borrow_mut().as_mut().unwrap().1.flush().unwrap();
        let rows: Vec<Value> = fs::read_to_string(&h.trace).unwrap().lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        let prepare = rows.iter().position(|r| r["event"] == "started" && r["function"] == "PlayerRigSystem.Prepare").unwrap();
        let ready = rows.iter().position(|r| r["event"] == "audicle_stage" && r["stage"] == "Ready").unwrap();
        assert!(prepare < ready);
        assert_eq!(rows[prepare]["audicle"], event);
        assert_eq!(h.entities.len(), 0);
        assert!(h.graphics.texture_owner().is_none());
        fs::remove_file(&h.trace).unwrap();
    }
    fn host() -> Host {
        Host::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    #[test]
    fn numeric_surface_height_batches_match_scalar_routes(){
        let mut h=host();h.invoke("CaveSystem.Configure",Value::Null).unwrap();h.stack.push((1,"PlayerRigSystem.Advance".into()));
        let r=crate::terrain::RADIUS;let camera=[20.,r+15.,-20.];let points:Vec<_>=[[-240.,r+40.,0.],[0.,r+2.,0.],[12.,r-8.,-8.],[25.,r-64.,-272.]].into_iter().flat_map(|p|(0..4).map(move|k|[p[0]+k as f64*0.12,p[1],p[2]])).collect();
        let expected:Vec<_>=points.iter().map(|p|h.invoke("PlanetSurfaceSystem.Height",json!({"position":p,"camera_eye":camera})).unwrap()["height"].as_f64().unwrap().to_bits()).collect();
        let got=h.surface_heights_numeric(&points,camera).unwrap();assert_eq!(got.into_iter().map(f64::to_bits).collect::<Vec<_>>(),expected);
    }
    #[test]
    fn terrain_direction_height_does_not_use_cave_player_support() {
        let mut host = host();
        host.invoke("CaveSystem.Configure", Value::Null).unwrap();
        host.stack.push((1, "PlanetNavigationSystem.Step".into()));
        let height = host.invoke("PlanetSurfaceSystem.Height", json!({"position":[0.,1.,0.]})).unwrap();
        assert!(height["height"].as_f64().unwrap().abs() < 1., "{height}");
    }
    #[test]
    fn persistent_scene_outlives_finish_cancel_and_replay() {
        let mut host = traced_host();
        host.stack.push((1, "ApplicationSystem.Run".into()));
        let input = json!({"procedure":"hello_triangle"});
        host.invoke("CueSystem.Begin", input.clone()).unwrap();
        let visual = host.graphics.prepared_entity();
        assert!(host.invoke("CueSystem.Begin", input.clone()).is_err());
        let status = host.invoke("CueSystem.Advance", Value::Null).unwrap();
        assert_eq!(status["finished"], 1);
        assert_eq!(host.entities.len(), 1);
        assert!(host.active_audicle.is_none());
        host.invoke("AppearanceSystem.Pump", status["visual"].clone())
            .unwrap();
        host.invoke("CueSystem.Begin", input.clone()).unwrap();
        assert_eq!(visual, host.graphics.prepared_entity());
        host.invoke("CueSystem.Cancel", Value::Null).unwrap();
        assert_eq!(visual, host.graphics.prepared_entity());
        host.invoke("CueSystem.Begin", input).unwrap();
        host.invoke("CueSystem.Advance", Value::Null).unwrap();
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        let status = host.invoke("CueSystem.Status", Value::Null).unwrap();
        assert_eq!(status["finished"], 2);
        assert_eq!(status["cancelled"], 1);
        assert_eq!(host.entities.len(), 0);
        assert!(host.graphics.prepared_entity().is_none());
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "native_prepared")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "native_released")
                .count(),
            1
        );
        fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn interactive_preparation_failure_cancels_and_shutdown_releases_scene() {
        let mut host = traced_host();
        host.graphics.fail_prepare = true;
        host.stack.push((1, "ApplicationSystem.Run".into()));
        assert!(
            host.invoke("CueSystem.Begin", json!({"procedure":"hello_triangle"}))
                .is_err()
        );
        assert!(host.active_audicle.is_none());
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        assert_eq!(host.entities.len(), 0);
        host.graphics.fail_prepare = false;
        host.invoke("CueSystem.Begin", json!({"procedure":"hello_triangle"}))
            .unwrap();
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        assert_eq!(host.entities.len(), 0);
        assert!(host.active_audicle.is_none());
        fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn interactive_hooks_require_declared_caller_and_correct_payloads() {
        let mut host = host();
        assert!(
            host.permitted("CueSystem.Begin", &json!({"procedure":"hello_triangle"}))
                .is_err()
        );
        host.stack.push((1, "CueSystem.Status".into()));
        assert!(
            host.permitted("AudicleSystem.Create", &Value::Null)
                .is_err()
        );
        assert!(!check_payload(
            "WindowFrame",
            &json!({"closed":false,"drawn":true,"keys":[true]})
        ));
        assert!(!check_payload(
            "CueStatus",
            &json!({"active":true,"visual":{"entity":0}})
        ));
    }
    #[test]
    fn flight_holds_one_event_until_idle_and_finishes_before_scene_cleanup() {
        let mut host = traced_host();
        host.stack.push((1, "ApplicationSystem.Run".into()));
        host.invoke("CueSystem.Begin", json!({"procedure":"hello_cube"}))
            .unwrap();
        let scene = host.invoke("CueSystem.Advance", Value::Null).unwrap();
        let mut step = json!({"entity":scene["visual"]["entity"],"dt":0.02,"move":[true,false,false,false,false,false],"look":[0,0],"reset":false});
        host.invoke("FlightSystem.Step", step.clone()).unwrap();
        let event = host.active_audicle;
        assert!(event.is_some());
        host.invoke("FlightSystem.Step", step.clone()).unwrap();
        assert_eq!(host.active_audicle, event);
        step["move"] = json!([false, false, false, false, false, false]);
        host.invoke("FlightSystem.Step", step.clone()).unwrap();
        assert!(host.active_audicle.is_none());
        step["reset"] = json!(true);
        host.invoke("FlightSystem.Step", step).unwrap();
        host.invoke("FlightSystem.Stop", Value::Null).unwrap();
        assert!(host.active_audicle.is_none());
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        assert_eq!(host.entities.len(), 0);
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let steps: Vec<_> = rows
            .iter()
            .filter(|r| r["event"] == "flight_step")
            .collect();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0]["audicle"], steps[1]["audicle"]);
        assert_ne!(steps[1]["audicle"], steps[2]["audicle"]);
        fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn view_changes_cannot_bypass_the_active_event_requirement() {
        let mut host = host();
        host.stack.push((1, "AppearanceSystem.View".into()));
        assert!(
            host.permitted(
                "GraphicsConnectionSystem.SetView",
                &json!({"entity":1,"triangles":[]})
            )
            .unwrap_err()
            .contains("active Audicle")
        );
        assert!(!check_payload(
            "TriangleBatch",
            &json!({"entity":1,"triangles":[[1,2,3]]})
        ));
    }
    #[test]
    fn configured_route_is_permitted() {
        let mut host = host();
        host.stack.push((1, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted("NativeInventorySystem.Probe", &Value::Null)
                .is_ok()
        );
    }
    #[test]
    fn undeclared_cross_system_call_is_rejected() {
        let mut host = host();
        host.stack.push((1, "NativeInventorySystem.Probe".into()));
        assert!(
            host.permitted("DiagnosticsSystem.Report", &Value::Null)
                .unwrap_err()
                .contains("undeclared")
        );
    }
    #[test]
    fn disabled_system_is_rejected() {
        let mut host = host();
        host.enabled.insert("DiagnosticsSystem".into(), false);
        assert!(
            host.permitted("DiagnosticsSystem.Report", &Value::Null)
                .is_err()
        );
    }
    #[test]
    fn unscheduled_host_entry_is_rejected() {
        assert!(
            host()
                .permitted("NativeInventorySystem.Probe", &Value::Null)
                .is_err()
        );
    }
    #[test]
    fn physical_computer_entry_accepts_production_resume_request() {
        for request in [json!({"eyewear":null,"computer_screen":true}), json!({"eyewear":"ar","computer_screen":false}), Value::Null] {
            assert!(check_payload("WorkstationEntry", &request), "{request}");
        }
        for request in [json!({"computer_screen":"true"}), json!({"computer_screen":null}), json!({"eyewear":"invalid","computer_screen":true}), json!({"unexpected":true})] {
            assert!(!check_payload("WorkstationEntry", &request), "{request}");
        }
    }
    #[test]
    fn wrong_input_type_is_rejected() {
        assert!(
            host()
                .permitted("DiagnosticsSystem.Report", &json!(42))
                .is_err()
        );
    }
    #[test]
    fn jack_route_requires_declared_audio_caller() {
        let mut host = host();
        host.stack.push((1, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted("JackConnectionSystem.CheckConnection", &Value::Null)
                .is_err()
        );
        host.stack.pop();
        host.stack.push((2, "DiagnosticsSystem.CheckAudio".into()));
        assert!(
            host.permitted("JackConnectionSystem.CheckConnection", &Value::Null)
                .is_ok()
        );
        host.enabled.insert("JackConnectionSystem".into(), false);
        assert!(
            host.permitted("JackConnectionSystem.CheckConnection", &Value::Null)
                .is_err()
        );
    }
    #[test]
    fn halt_circuit_is_respected() {
        let mut host = host();
        host.halt = true;
        assert!(
            host.permitted("DiagnosticsSystem.Report", &Value::Null)
                .is_err()
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn excavation_save_reload_preserves_surface_fragment_mass_and_motion() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = std::env::temp_dir().join(format!("mirror-excavation-{}", std::process::id()));
        let mut h = Host::load_paths(root, &output).unwrap();
        // This regression explicitly selects persistence in an isolated fixture;
        // ordinary scene defaults must not silently decide whether a save test saves.
        h.invoke("CaveSystem.Configure", Value::Null).unwrap();
        let fixture=output.join("fixture-assets");
        fs::create_dir_all(fixture.join("Procedures")).unwrap();
        let mut config=read(root,"Procedures/terrain_session.json").unwrap();
        config["persist_excavation"]=json!(true);
        fs::write(fixture.join("Procedures/terrain_session.json"),serde_json::to_vec(&config).unwrap()).unwrap();
        h.root=fixture.clone();
        // Exercise the retained excavation API through an explicit test route;
        // the current navigation controller does not dispatch gesture excavation.
        for target in ["PlanetSurfaceSystem.Excavate", "PlanetSurfaceSystem.Advance"] {
            h.hooks.get_mut("PlanetNavigationSystem.Step").unwrap().calls.insert(target.into());
        }
        h.stack.push((1, "PlanetNavigationSystem.Step".into()));
        let event = h.invoke("AudicleSystem.Create", Value::Null).unwrap();
        h.invoke("AudicleSystem.Ready", event.clone()).unwrap();
        h.invoke("AudicleSystem.Go", event.clone()).unwrap();
        let request = json!({"position":[0.,6371002.,0.],"forward":[0.,-1.,0.]});
        assert_eq!(
            h.invoke("PlanetSurfaceSystem.Excavate", request).unwrap()["changed"],
            true
        );
        let frame = h
            .invoke("PlanetSurfaceSystem.Advance", Value::Null)
            .unwrap();
        assert_eq!(frame["fragments"], 8);
        assert_eq!(frame["physics"]["halted"], false);
        assert!(frame["removed_volume"].as_f64().unwrap() > 5.);
        let outside = h
            .invoke(
                "PlanetSurfaceSystem.Excavate",
                json!({"position":[100.,6371002.,0.],"forward":[0.,-1.,0.]}),
            )
            .unwrap();
        assert_eq!(outside["changed"], false);
        let overlapping = h
            .invoke(
                "PlanetSurfaceSystem.Excavate",
                json!({"position":[0.,6371002.,0.],"forward":[0.,-1.,0.]}),
            )
            .unwrap();
        assert_eq!(overlapping["changed"], false);
        let unchanged = h.invoke("PlanetSurfaceSystem.Frame", Value::Null).unwrap();
        assert_eq!(unchanged["debris"], frame["debris"]);
        assert_eq!(unchanged["removed_mass"], frame["removed_mass"]);

        h.invoke("AudicleSystem.Finish", event).unwrap();
        h.stack.pop();
        h.stack.push((2, "PlanetNavigationSystem.Stop".into()));
        h.invoke("PlanetSurfaceSystem.Save", Value::Null).unwrap();
        h.stack.pop();
        let disk: Value =
            serde_json::from_slice(&fs::read(output.join("planet/surface.json")).unwrap()).unwrap();
        let mass: f64 = disk["particles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["mass"].as_f64().unwrap())
            .sum();
        assert!((mass - frame["removed_mass"].as_f64().unwrap()).abs() < 0.01);
        let mut reloaded = Host::load_paths(root, &output).unwrap();
        reloaded.invoke("CaveSystem.Configure", Value::Null).unwrap();
        reloaded.root=fixture;
        reloaded
            .stack
            .push((1, "PlanetNavigationSystem.Step".into()));
        let replay = reloaded
            .invoke("PlanetSurfaceSystem.Frame", Value::Null)
            .unwrap();
        assert_eq!(replay["edits"], frame["edits"]);
        assert_eq!(replay["debris"], frame["debris"]);
        assert_eq!(replay["removed_mass"], frame["removed_mass"]);
        reloaded.stack.pop();
        reloaded
            .stack
            .push((2, "PlanetNavigationSystem.Stop".into()));
        reloaded
            .invoke("PlanetSurfaceSystem.Save", Value::Null)
            .unwrap();
        let replay_disk: Value =
            serde_json::from_slice(&fs::read(output.join("planet/surface.json")).unwrap()).unwrap();
        assert_eq!(
            replay_disk["particles"][0]["rest"],
            disk["particles"][0]["rest"]
        );
        reloaded.stack.pop();
        drop(reloaded);
        drop(h);
        fs::remove_dir_all(output).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn workshop_soil_spade_reaches_and_holds_contact_without_claiming_fracture() {
        let mut h = traced_host();
        let report = h.invoke("WorkshopSystem.Benchmark", json!({"case":"soil_spade_contact","frames":180})).unwrap();
        let physics = &report["physics"];
        let probe = &physics["soil_probe"];
        assert_eq!(physics["halted"], false, "{physics}");
        assert_eq!(physics["verified_intersections"], 0);
        assert!(probe["peak_contact_pairs"].as_u64().unwrap() > 0);
        assert!(probe["peak_blade_force_newtons"].as_f64().unwrap() > 0.01, "contact must act on the blade itself: {probe}");
        let actual = probe["accepted_blade_drop_m"].as_f64().unwrap();
        let target = probe["target_drop_m"].as_f64().unwrap();
        assert!(actual > 0.1 && actual < target - 0.05, "the force-driven blade must reach the soil and remain constrained: {probe}");
        assert!(probe["maximum_soil_displacement_m"].as_f64().unwrap() > 0.000001);
        assert_eq!(probe["fracture_implemented"], false);
        assert_eq!(h.ogc.ownership("PhysicsSystem")["configured"], false);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn workshop_benchmark_tracks_real_material_steps_and_releases_contacts() {
        let mut h = traced_host();
        let report = h
            .invoke(
                "WorkshopSystem.Benchmark",
                json!({"case":"cloth_sphere","frames":3}),
            )
            .unwrap();
        assert_eq!(report["physics"]["frames"], 3);
        assert_eq!(report["physics"]["halted"], false);
        assert!(report["physics"]["simulated_seconds"].as_f64().unwrap() > 0.049);
        assert_eq!(h.active_audicle, None);
        assert_eq!(h.entities.len(), 0);
        assert!(h.physics.scene.is_none());
        assert_eq!(h.ogc.ownership("PhysicsSystem")["configured"], false);
        let rows: Vec<Value> = fs::read_to_string(&h.trace)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(
            rows.iter().filter(|r| r["event"] == "physics_step").count(),
            3
        );
        assert!(
            rows.iter()
                .filter(|r| r["event"] == "physics_step")
                .all(|r| r["audicle"].is_u64())
        );
        fs::remove_file(&h.trace).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn contact_solve_resists_an_inward_material_target_without_collapsing_clearance() {
        let mut h = traced_host();
        let mut positions = json!([
            [-0.1, 0.005, -0.1],
            [0.1, 0.005, -0.1],
            [0., 0.005, 0.1],
            [-1., 0., -1.],
            [1., 0., -1.],
            [1., 0., 1.],
            [-1., 0., 1.]
        ]);
        h.stack.push((1, "PhysicsSystem.Configure".into()));
        h.invoke("OGCConnectionSystem.Configure",json!({"positions":positions,"contact_radius":0.01,"contact_stiffness":5000.,"surfaces":[{"start":0,"count":3,"triangles":[[0,1,2]],"motion":0,"self_collision":false,"friction":0.},{"start":3,"count":4,"triangles":[[0,2,1],[0,3,2]],"motion":2,"self_collision":false,"friction":0.}]})).unwrap();
        h.stack.pop();
        h.stack.push((2, "PhysicsSystem.Advance".into()));
        for _ in 0..20 {
            let mut target = positions.clone();
            for i in 0..3 {
                target[i][1] = json!(-0.01);
            }
            let result=h.invoke("OGCConnectionSystem.Project",json!({"positions":target,"previous":positions,"inverse_mass":[25.,25.,25.,0.,0.,0.,0.],"dt":1./240.})).unwrap();
            positions = result["positions"].clone();
        }
        assert!((0..3).all(|i| positions[i][1].as_f64().unwrap() > 0.001));
        assert!((3..7).all(|i| positions[i][1] == 0.));
        fs::remove_file(&h.trace).unwrap();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn ogc_active_mask_omits_buried_query_vertices() {
        let mut h=traced_host();
        let positions=json!([[-1.,1.,-1.],[1.,1.,-1.],[0.,1.,1.],[0.,0.005,0.],[-1.,0.,-1.],[1.,0.,-1.],[0.,0.,1.]]);
        let mut scene=json!({"positions":positions,"contact_radius":0.01,"contact_stiffness":5000.,"surfaces":[
            {"start":0,"count":4,"triangles":[[0,1,2]],"motion":0,"self_collision":false},
            {"start":4,"count":3,"triangles":[[0,2,1]],"motion":2,"self_collision":false}
        ]});
        let request=json!({"positions":positions,"previous":positions,"inverse_mass":[0.,0.,0.,0.,0.,0.,0.],"dt":1./240.,"all_responses":true});
        h.stack.push((1,"PhysicsSystem.Configure".into()));
        h.invoke("OGCConnectionSystem.Configure",scene.clone()).unwrap();
        h.stack.pop();h.stack.push((2,"PhysicsSystem.Advance".into()));
        let unmasked=h.invoke("OGCConnectionSystem.Project",request.clone()).unwrap();
        assert!(unmasked["all_responses"][3][1].as_f64().unwrap().abs()>0.01);
        scene["surfaces"][0]["active_vertices"]=json!([true,true,true,false]);
        h.stack.pop();h.stack.push((3,"PhysicsSystem.Configure".into()));
        h.invoke("OGCConnectionSystem.Configure",scene).unwrap();
        h.stack.pop();h.stack.push((4,"PhysicsSystem.Advance".into()));
        let masked=h.invoke("OGCConnectionSystem.Project",request).unwrap();
        assert_eq!(masked["all_responses"][3][1],0.);
        assert_eq!(masked["all_responses"].as_array().unwrap().len(),7);
        h.stack.pop();h.stack.push((5,"PhysicsSystem.Release".into()));
        h.invoke("OGCConnectionSystem.Release",Value::Null).unwrap();
        h.stack.pop();fs::remove_file(&h.trace).unwrap();
    }
    include!("../Workstation/PianoRollIntegrationTests.rs");
    include!("../Workstation/MobileWorkflowIntegrationTests.rs");
    include!("../Workstation/FullscreenPanelIntegrationTests.rs");
    include!("../Workstation/PluginIntegrationTests.rs");
    include!("AudioLibraryIntegrationTests.rs");
    include!("WorkstationExportTests.rs");
    include!("WorkstationBusTests.rs");
    include!("WorkstationViewportTests.rs");
    include!("TimelineBarIntegrationTests.rs");
    include!("WorkstationPerformanceTests.rs");
    #[test] fn nightworld_registered_portal_holds_loading_then_preserves_coordinates(){
        let mut h=traced_host();h.invoke("PlanetNavigationSystem.Configure",Value::Null).unwrap();
        let c=crate::terrain::nightworld::shaft();let p=crate::terrain::underground::world([c[0],-94.7+1.84,c[2]]);
        h.environment.place_portal_test_source(p);
        h.stack.push((991,"ApplicationSystem.Run".into()));
        let input=json!({"entity":1,"dt":0.05,"move":[false,false,false,false,false,false],"look":[0.,0.],"reset":false,"boost":false,"defer_presentation":true});
        let first=h.invoke("PlanetNavigationSystem.Step",input.clone()).unwrap();assert_eq!(first["realm"],"overworld");assert!(first["portal_loading"].as_f64().unwrap()>0.);
        for _ in 0..14 {let hold=h.invoke("PlanetNavigationSystem.Step",input.clone()).unwrap();assert_eq!(hold["position"],first["position"]);assert_eq!(hold["realm"],"overworld");}
        let mut arrived=Value::Null;
        for _ in 0..4 {let v=h.invoke("PlanetNavigationSystem.Step",input.clone()).unwrap();if v["realm"]=="nightworld" {arrived=v;break;}}
        assert_eq!(arrived["realm"],"nightworld");assert_eq!(arrived["position"][0],json!(p[0]));assert_eq!(arrived["position"][2],json!(p[2]));assert!(arrived["up"][1].as_f64().unwrap()< -0.99);assert_eq!(arrived["portal_loading"],0.);
        let _=fs::remove_file(&h.trace);
    }
    fn traced_host() -> Host {
        let mut host = host();
        host.trace = std::env::temp_dir().join(format!("wormwood-test-{}.jsonl", host.run_id));
        host.graphics.test_driver = true;
        host
    }
    #[test]
    fn workstation_backend_registered_routes_render_pre_master_pcm() {
        let mut host = traced_host();
        let edits = [
            ("PutTrack",json!({"id":"track","name":"Audio","kind":"audio"})),
            ("PutSource",json!({"id":"source","sample_rate":8000,"channels":1,"samples":[1.,-1.]})),
            ("PutClip",json!({"id":"clip","track":"track","kind":"audio","source":"source",
                "start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.00025})),
            ("SetTrackMix",json!({"id":"track","output_bus_l":"L","output_bus_r":"FF"})),
        ];
        for (hook,mut input) in edits {
            input["document_revision"] = host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
            host.invoke(&format!("WorkstationDataSystem.{hook}"),input).unwrap();
        }
        let revision=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
        let rendered=host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":revision,
            "sample_rate":8000,"start_frame":0,"frames":2,"master_gain":0.})).unwrap();
        assert_eq!(rendered["lsffr"],json!([[1.,0.,1.,0.],[-1.,0.,-1.,0.]]));
        assert_eq!(rendered["master_stereo"],json!([[0.,0.],[0.,0.]]));
        assert_eq!(rendered["export_tap"],"pre_stereo_master");
        assert_eq!(rendered["audio_device_connected"],false);
        assert!(host.active_audicle.is_none());
        assert!(host.audio.prepared_entity().is_none());
        assert_eq!(host.entities.len(),0);
        host.enabled.insert("WorkstationDataSystem".into(),false);
        assert!(host.invoke("WorkstationDataSystem.Snapshot",Value::Null).is_err());
        drop(host.trace_writer.borrow_mut().take());
        fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn workstation_registered_lifecycle_reaches_view_and_releases() {
        let mut host = traced_host();
        let begin = host.invoke("AudioWorkstationSystem.Begin", Value::Null).unwrap();
        assert_eq!(begin["open"], true);
        let step = json!({"dt":1.0,"aspect":390.0/844.0,"viewport":[390.,844.],
            "framebuffer":[1170.,2532.],"canvas_scale":1.0,"keys":[false,false,false,false,false]});
        let state = host.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert_eq!(state["animating"], false);
        assert!(state["panels"].as_array().unwrap().iter().all(|panel| panel["open"] == true));
        host.invoke("AudioWorkstationSystem.SetPanel", json!({"edge":"bottom","expanded":true})).unwrap();
        let expanded = host.invoke("AudioWorkstationSystem.Step", step).unwrap();
        assert_eq!(expanded["panels"][1]["expanded"], true);
        assert_eq!(host.invoke("AudioWorkstationSystem.Close", Value::Null).unwrap(), Value::Null);
        assert_eq!(host.entities.len(), 0);
        assert!(host.graphics.prepared_entity().is_none());
        assert!(host.active_audicle.is_none());
        fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn workstation_source_spheres_follow_registered_pcm_publication_and_gain() {
        let mut host=traced_host();
        for (hook,mut input) in [
            ("PutTrack",json!({"id":"sphere-source","name":"Sphere source","kind":"audio"})),
            ("PutSource",json!({"id":"sphere-pcm","sample_rate":8000,"channels":1,"samples":vec![0.25;128]})),
            ("PutClip",json!({"id":"sphere-clip","track":"sphere-source","kind":"audio","source":"sphere-pcm",
                "start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.016})),
        ] {
            input["document_revision"]=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
            host.invoke(&format!("WorkstationDataSystem.{hook}"),input).unwrap();
        }
        let revision=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
        host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":revision,
            "sample_rate":8000,"start_frame":0,"frames":128,"publish_visualization":true})).unwrap();
        host.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
        let step=json!({"dt":1.,"aspect":16./9.,"viewport":[1280.,720.],"keys":vec![false;5],"spatial_view":true});
        let presented=host.invoke("AudioWorkstationSystem.Step",step.clone()).unwrap();
        assert_eq!(presented["spatial_error"],Value::Null);
        assert_eq!(presented["spatial_mix"]["sources"][0]["available"],true);
        assert_eq!(presented["spatial_mix"]["sources"][0]["fresh"],true);
        assert_eq!(presented["piano_roll"]["open"],false);
        // Hit the projected source through the registered Step route, then
        // verify that the gesture changes the backend fader, not MIDI data.
        use crate::workstation::{panel_layout::{Options,Viewport},panel_motion,
            piano_roll_scene,spatial_mix};
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
        let mut model=spatial_mix::State::default();
        model.sync(&snapshot,1.).unwrap();
        let mut motion=panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        let view=Viewport {width:1280.,height:720.,canvas_scale:1.};
        let rect=piano_roll_scene::canvas(view,&motion.snapshot(),Options::default()).unwrap();
        let sphere=model.presentation(rect,view);
        let center=&sphere["spheres"][0]["center"];
        let pointer=[640.+center[0].as_f64().unwrap()*720./1.3,
            360.-center[1].as_f64().unwrap()*720./1.3];
        let mut gesture=step.clone();
        gesture["dt"]=json!(0.);
        gesture["editor"]=json!({"pointer":pointer,"left":true});
        gesture["spatial_view"]=json!(false);
        host.invoke("AudioWorkstationSystem.Step",gesture.clone()).unwrap();
        gesture["spatial_view"]=json!(true);
        let held=host.invoke("AudioWorkstationSystem.Step",gesture.clone()).unwrap();
        assert_eq!(held["spatial_mix"]["interacting"],false,
            "a held click from the arrangement must not become a new sphere press");
        gesture["editor"]["left"]=json!(false);
        host.invoke("AudioWorkstationSystem.Step",gesture.clone()).unwrap();
        gesture["editor"]["left"]=json!(true);
        let grabbed=host.invoke("AudioWorkstationSystem.Step",gesture.clone()).unwrap();
        assert_eq!(grabbed["spatial_mix"]["interacting"],true);
        assert_eq!(grabbed["piano_roll"]["open"],false);
        gesture["editor"]["pointer"]=json!([pointer[0],pointer[1]-20.]);
        let dragged=host.invoke("AudioWorkstationSystem.Step",gesture.clone()).unwrap();
        assert!(dragged["spatial_mix"]["sources"][0]["gain"].as_f64().unwrap()<1.);
        assert!(dragged["spatial_error"].is_null());
        gesture["editor"]["left"]=json!(false);
        let released=host.invoke("AudioWorkstationSystem.Step",gesture).unwrap();
        assert_eq!(released["spatial_mix"]["interacting"],false);
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
        assert_eq!(snapshot["tracks"].as_array().unwrap().len(),1);
        assert_eq!(snapshot["clips"].as_array().unwrap().len(),1);
        let revision=snapshot["revision"].clone();
        host.invoke("WorkstationDataSystem.SetTrackMix",json!({"document_revision":revision,"id":"sphere-source","gain":0.5})).unwrap();
        let revision=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
        let rendered=host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":revision,
            "sample_rate":8000,"start_frame":0,"frames":128,"publish_visualization":true})).unwrap();
        assert_eq!(rendered["visualization"]["tracks"][0]["rms_post_gain"],0.125);
        let changed=host.invoke("AudioWorkstationSystem.Step",step).unwrap();
        assert_eq!(changed["spatial_mix"]["sources"][0]["gain"],0.5);
        assert!(changed["spatial_error"].is_null());
        host.invoke("AudioWorkstationSystem.Close",Value::Null).unwrap();
        assert!(host.active_audicle.is_none());
        assert_eq!(host.entities.len(),0);
        drop(host.trace_writer.borrow_mut().take());
        fs::remove_file(&host.trace).unwrap();
    }
    fn procedure_host(value: Value) -> Host {
        let mut host = traced_host();
        host.root = std::env::temp_dir().join(format!("wormwood-procedure-{}", host.run_id));
        let folder = host.root.join("Procedures/Cues");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("authored.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        host
    }
    fn default_procedure() -> Value {
        serde_json::from_str(include_str!("../Procedures/Cues/hello_triangle.json")).unwrap()
    }
    fn remove_procedure_host(host: Host) {
        fs::remove_dir_all(&host.root).unwrap();
        fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn authored_cue_data_reaches_entity_creation_through_registered_loader() {
        let mut definition = default_procedure();
        definition["visual"]["vertices"][0][3] = json!(0.37);
        definition.as_object_mut().unwrap().remove("sound");
        let mut host = procedure_host(definition.clone());
        host.audio_enabled = true;
        let result = host
            .invoke("CueSystem.Run", json!({"procedure":"authored"}))
            .unwrap();
        assert_eq!(result["procedure"], "authored");
        assert!(result["audio"].is_null());
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let created = rows
            .iter()
            .find(|r| r["event"] == "entity_created")
            .unwrap();
        assert!((created["appearance"]["vertices"][0][3].as_f64().unwrap() - 0.37).abs() < 0.00001);
        assert!(rows.iter().any(|r| r["event"] == "procedure_loaded"
            && r["function"] == "CueProcedureSystem.Load"
            && r["audicle"].is_number()));
        assert!(!rows.iter().any(|r| r["event"] == "audio_prepared"));
        assert!(!host.startup_steps.iter().any(|r| r == "CueSystem.Run"));
        remove_procedure_host(host);
    }
    #[test]
    fn invalid_cue_cannot_create_entities_or_reach_ready() {
        let mut definition = default_procedure();
        definition["calls"] = json!(["NativeInventorySystem.Probe"]);
        let mut host = procedure_host(definition);
        assert!(
            host.invoke("CueSystem.Run", json!({"procedure":"authored"}))
                .unwrap_err()
                .contains("unknown cue field")
        );
        let text = fs::read_to_string(&host.trace).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(!rows.iter().any(|r| r["event"] == "entity_created"
            || r["stage"] == "Ready"
            || r["stage"] == "Go"));
        assert!(rows.iter().any(|r| r["event"] == "audicle_cancelled"));
        assert!(host.active_audicle.is_none());
        remove_procedure_host(host);
    }
    #[test]
    fn cue_reference_rejects_paths_and_loader_requires_declared_caller() {
        let mut host = host();
        for name in ["../LICENSE", "/tmp/cue", "", "a/b", "a\\b"] {
            assert!(
                host.permitted("CueSystem.Run", &json!({"procedure":name}))
                    .is_err()
            );
        }
        host.stack.push((123, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted(
                "CueProcedureSystem.Load",
                &json!({"procedure":"hello_triangle"})
            )
            .unwrap_err()
            .contains("undeclared call")
        );
    }
    fn startup_host() -> Host {
        let mut host = traced_host();
        host.output = std::env::temp_dir().join(format!("wormwood-startup-{}", host.run_id));
        fs::create_dir_all(&host.output).unwrap();
        host.trace = host.output.join("execution.jsonl");
        host
    }
    #[test]
    fn startup_runs_once_and_is_not_repeated_between_cues() {
        let mut host = startup_host();
        host.startup_steps = vec!["DiagnosticsSystem.ValidateRuntime".into()];
        let report = host.startup().unwrap();
        assert_eq!(report["started"], true);
        assert_eq!(report["completed_steps"].as_array().unwrap().len(), 1);
        for _ in 0..2 {
            host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap();
        }
        assert!(
            host.startup()
                .unwrap_err()
                .contains("already been attempted")
        );
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "started"
                    && r["function"] == "DiagnosticsSystem.ValidateRuntime")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "audicle_stage" && r["stage"] == "Finished")
                .count(),
            2
        );
        fs::remove_dir_all(&host.output).unwrap();
    }
    #[test]
    fn startup_failure_stops_later_steps_and_cannot_be_retried() {
        let mut host = startup_host();
        host.startup_steps = vec![
            "DiagnosticsSystem.ValidateRuntime".into(),
            "NativeInventorySystem.Probe".into(),
            "DiagnosticsSystem.Report".into(),
        ];
        host.roots.push("NativeInventorySystem.Probe".into());
        host.enabled.insert("NativeInventorySystem".into(), false);
        assert!(
            host.startup()
                .unwrap_err()
                .contains("NativeInventorySystem.Probe")
        );
        assert!(host.startup().is_err());
        let report: Value =
            serde_json::from_slice(&fs::read(host.output.join("startup.json")).unwrap()).unwrap();
        assert_eq!(report["started"], false);
        assert_eq!(report["completed_steps"].as_array().unwrap().len(), 1);
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(
            !rows
                .iter()
                .any(|r| r["event"] == "started" && r["function"] == "DiagnosticsSystem.Report")
        );
        fs::remove_dir_all(&host.output).unwrap();
    }
    #[test]
    fn startup_plan_follows_systems_list_and_keeps_entries_separate() {
        let original = host();
        let root = std::env::temp_dir().join(format!("wormwood-startup-plan-{}", original.run_id));
        for dir in ["Systems", "BaseSystem", "Procedures"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for entry in fs::read_dir(original.root.join("Systems")).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, root.join("Systems").join(path.file_name().unwrap())).unwrap();
        }
        for name in [
            "BaseSystem/registry.json",
            "BaseSystem/circuit_breaker.json",
            "Procedures/functionregistry.json",
            "Procedures/systems.json",
        ] {
            fs::copy(original.root.join(name), root.join(name)).unwrap();
        }
        let mut native = read(&root, "Systems/NativeInventorySystem.json").unwrap();
        native["init_steps"] = json!(["Probe"]);
        fs::write(
            root.join("Systems/NativeInventorySystem.json"),
            serde_json::to_vec(&native).unwrap(),
        )
        .unwrap();
        let mut schedule = read(&root, "Procedures/systems.json").unwrap();
        let order = schedule["systems_order"].as_array_mut().unwrap();
        let index = order
            .iter()
            .position(|v| v == "NativeInventorySystem.json")
            .unwrap();
        let native = order.remove(index);
        order.push(native);
        fs::write(
            root.join("Procedures/systems.json"),
            serde_json::to_vec(&schedule).unwrap(),
        )
        .unwrap();
        let loaded = Host::load(&root).unwrap();
        assert_eq!(
            loaded.startup_steps,
            vec![
                "DiagnosticsSystem.ValidateRuntime",
                "DiagnosticsSystem.Report",
                "DiagnosticsSystem.CheckAudio",
                "PlanetNavigationSystem.Configure",
                "CaveSystem.Configure",
                "NativeInventorySystem.Probe"
            ]
        );
        assert!(!loaded.startup_steps.iter().any(|h| h == "CueSystem.Run"));
        assert!(loaded.roots.iter().any(|h| h == "CueSystem.Run"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn window_close_cancels_cue_before_or_after_first_frame_and_allows_reuse() {
        for frames in [0, 1] {
            let mut host = traced_host();
            host.graphics.close_after = Some(frames);
            let report = host
                .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap();
            assert_eq!(report["stage"], "Cancelled");
            assert_eq!(report["reason"], "window_closed");
            assert_eq!(report["presentation"]["frames"], frames);
            assert_eq!(host.entities.len(), 0);
            assert!(host.active_audicle.is_none());
            assert!(host.graphics.prepared_entity().is_none());
            let rows: Vec<Value> = fs::read_to_string(&host.trace)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            assert_eq!(
                rows.iter()
                    .filter(|r| r["event"] == "audicle_cancelled")
                    .count(),
                1
            );
            assert!(!rows.iter().any(|r| r["stage"] == "Finished"));
            assert_eq!(
                rows.iter()
                    .filter(|r| r["event"] == "entity_removed")
                    .count(),
                1
            );
            assert_eq!(
                rows.iter()
                    .filter(|r| r["event"] == "native_released")
                    .count(),
                1
            );
            host.graphics.close_after = None;
            assert_eq!(
                host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                    .unwrap()["stage"],
                "Finished"
            );
            fs::remove_file(&host.trace).unwrap();
        }
    }
    #[test]
    fn cancellation_payload_does_not_accept_empty_normal_presentation_or_unknown_reason() {
        assert!(!check_payload(
            "Presentation",
            &json!({"backend":"OpenGL","frames":3,"end_reason":"ignored_failure"})
        ));
        assert!(!check_payload(
            "Presentation",
            &json!({"backend":"OpenGL","frames":0})
        ));
        assert!(check_payload(
            "Presentation",
            &json!({"backend":"OpenGL","frames":0,"end_reason":"window_closed"})
        ));
        assert!(!check_payload(
            "CueResult",
            &json!({"mode":"cue","stage":"Cancelled","reason":"ignored_failure","entities_remaining":0})
        ));
    }
    #[test]
    fn visual_cue_traces_lifecycle_and_removes_resources() {
        let mut host = traced_host();
        let result = host
            .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
            .unwrap();
        assert_eq!(result["stage"], "Finished");
        assert_eq!(result["presentation"]["backend"], "TestDriver");
        assert!(host.active_audicle.is_none());
        assert_eq!(host.entities.len(), 0);
        assert_eq!(host.graphics.prepared_entity(), None);
        let rows: Vec<Value> = std::fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let stages: Vec<&str> = rows
            .iter()
            .filter(|e| e["event"] == "audicle_stage")
            .map(|e| e["stage"].as_str().unwrap())
            .collect();
        assert_eq!(stages, vec!["Preparing", "Ready", "Go", "Finished"]);
        let ready = rows
            .iter()
            .position(|e| e["event"] == "audicle_stage" && e["stage"] == "Ready")
            .unwrap();
        let prepared = rows
            .iter()
            .position(|e| e["event"] == "native_prepared")
            .unwrap();
        let drawn = rows
            .iter()
            .position(|e| e["event"] == "frame_presented")
            .unwrap();
        let go = rows
            .iter()
            .position(|e| e["event"] == "audicle_stage" && e["stage"] == "Go")
            .unwrap();
        assert!(prepared < ready && ready < go && go < drawn);
        assert!(
            rows.iter()
                .filter(|e| e["event"] == "frame_presented")
                .all(|e| e["audicle"].as_u64().is_some())
        );
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn presentation_failure_cleans_up_and_cancels() {
        let mut host = traced_host();
        host.graphics.fail_present = true;
        let error = host
            .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
            .unwrap_err();
        assert!(error.contains("injected presentation failure"));
        assert_eq!(host.entities.len(), 0);
        assert!(host.active_audicle.is_none());
        assert_eq!(host.graphics.prepared_entity(), None);
        let text = std::fs::read_to_string(&host.trace).unwrap();
        assert!(text.contains("audicle_cancelled"));
        assert!(!text.contains("\"stage\":\"Finished\""));
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn host_can_reuse_cue_state_after_cancel_and_finish() {
        let mut host = traced_host();
        host.graphics.fail_present = true;
        assert!(
            host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .is_err()
        );
        host.graphics.fail_present = false;
        for _ in 0..2 {
            let report = host
                .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap();
            assert_eq!(report["stage"], "Finished");
            assert_eq!(host.entities.len(), 0);
            assert!(host.active_audicle.is_none());
            assert!(host.graphics.prepared_entity().is_none());
        }
        let text = std::fs::read_to_string(&host.trace).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "audicle_cancelled")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "audicle_stage" && r["stage"] == "Finished")
                .count(),
            2
        );
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn go_before_ready_is_rejected() {
        let mut host = traced_host();
        host.stack.push((999, "CueSystem.Run".into()));
        let cue = host.invoke("AudicleSystem.Create", Value::Null).unwrap();
        assert!(
            host.invoke("AudicleSystem.Go", cue.clone())
                .unwrap_err()
                .contains("invalid Audicle transition")
        );
        host.invoke("AudicleSystem.Cancel", cue).unwrap();
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn presentation_without_go_is_rejected() {
        let mut host = host();
        host.stack.push((999, "AppearanceSystem.Present".into()));
        assert!(
            host.permitted("GraphicsConnectionSystem.Present", &json!({"entity":1}))
                .unwrap_err()
                .contains("in Go")
        );
    }
    #[test]
    fn ios_audio_routes_enforce_go_and_caller_contract() {
        let mut host = host();
        host.stack.push((999, "CueSystem.Run".into()));
        assert!(
            host.permitted("IOSAudioConnectionSystem.Start", &json!({"entity":1}))
                .unwrap_err()
                .contains("in Go")
        );
        host.stack.pop();
        host.stack
            .push((999, "DiagnosticsSystem.CheckAudio".into()));
        assert!(
            host.permitted(
                "IOSAudioConnectionSystem.Prepare",
                &json!({"id":1,"kind":"sound","source":"diagnostic_sine_440"})
            )
            .unwrap_err()
            .contains("undeclared call")
        );
    }
    #[test]
    fn audio_start_without_go_and_undeclared_calls_are_rejected() {
        let mut host = host();
        host.stack.push((999, "CueSystem.Run".into()));
        assert!(
            host.permitted("JackConnectionSystem.Start", &json!({"entity":1}))
                .unwrap_err()
                .contains("in Go")
        );
        host.stack.pop();
        host.stack.push((1000, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted(
                "JackConnectionSystem.Prepare",
                &json!({"id":1,"kind":"sound","source":"diagnostic_sine_440"})
            )
            .unwrap_err()
            .contains("undeclared")
        );
        assert!(host.emit(json!({"event":"audio_completed"})).is_err());
    }
    #[test]
    fn unknown_entity_and_malformed_geometry_are_rejected() {
        let mut host = traced_host();
        host.stack.push((999, "AppearanceSystem.Prepare".into()));
        assert!(
            host.invoke("EntitySystem.Get", json!({"entity":123}))
                .unwrap_err()
                .contains("unknown Entity")
        );
        assert!(!check_payload(
            "EntitySpec",
            &json!({"kind":"visual","vertices":[[0,1]],"shader":"vertex_color"})
        ));
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn numeric_material_routes_preserve_audicle_and_caller_checks() {
        let mut host=traced_host();
        host.stack.push((1,"CueSystem.Run".into()));
        let spec=json!({"kind":"visual","shader":"planet_surface","vertices":[[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[]}});
        let id=host.invoke("EntitySystem.Create",spec.clone()).unwrap()["entity"].as_u64().unwrap();
        host.stack.pop();host.stack.push((2,"PlanetNavigationSystem.Step".into()));
        let skin=vec![[0.,0.,0.,0.2,0.3,0.4],[1.,0.,0.,0.2,0.3,0.4],[0.,1.,0.,0.2,0.3,0.4]];
        assert!(host.update_material_entity(id,spec["procedural"].clone(),skin.clone()).unwrap_err().contains("in Go"));
        let event=host.invoke("AudicleSystem.Create",Value::Null).unwrap();
        host.invoke("AudicleSystem.Ready",event.clone()).unwrap();host.invoke("AudicleSystem.Go",event.clone()).unwrap();
        host.update_material_entity(id,spec["procedural"].clone(),skin.clone()).unwrap();
        assert_eq!(host.entity_snapshot(id).unwrap().data()["procedural"]["debris"],json!(skin));
        host.invoke("AudicleSystem.Finish",event).unwrap();
        host.stack.pop();host.stack.push((3,"DiagnosticsSystem.Report".into()));
        assert!(host.entity_snapshot(id).unwrap_err().contains("undeclared"));
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn panel_input_diagnostic_is_allowed_only_for_workstation_driver() {
        let mut host=traced_host();
        for caller in ["AudioWorkstationSystem.Run","AudioWorkstationSystem.RunEmbedded"] {
            host.stack.push((901,caller.into()));
            host.emit(json!({"event":"panel_input_applied","keys":[true,false,false,false],"open":[true,false,false,false],"extensions":[0.,0.]})).unwrap();
            host.stack.pop();
        }
        for caller in ["DiagnosticsSystem.Report","GraphicsConnectionSystem.Pump","AudioWorkstationSystem.Step"] {
            host.stack.push((902,caller.into()));
            assert!(host.emit(json!({"event":"panel_input_applied"})).unwrap_err().contains("cannot emit"));
            host.stack.pop();
        }
        host.trace_writer.borrow_mut().as_mut().unwrap().1.flush().unwrap();
        let records:Vec<Value>=std::fs::read_to_string(&host.trace).unwrap().lines().map(|s|serde_json::from_str(s).unwrap()).collect();
        let emitted:Vec<_>=records.iter().filter(|r|r["event"]=="panel_input_applied").collect();
        assert_eq!(emitted.len(),2);
        assert_eq!(emitted[0]["function"],"AudioWorkstationSystem.Run");
        assert_eq!(emitted[1]["function"],"AudioWorkstationSystem.RunEmbedded");
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn diagnostics_cannot_forge_entity_lifecycle_records() {
        let mut host = host();
        host.stack.push((999, "DiagnosticsSystem.Report".into()));
        assert!(
            host.emit(json!({"event":"entity_created","entity":1}))
                .unwrap_err()
                .contains("cannot emit")
        );
    }
    #[test]
    fn preparation_failure_never_reaches_ready_or_go() {
        let mut host = traced_host();
        host.graphics.fail_prepare = true;
        assert!(
            host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap_err()
                .contains("injected preparation failure")
        );
        assert_eq!(host.entities.len(), 0);
        assert!(host.active_audicle.is_none());
        let rows: Vec<Value> = std::fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(!rows.iter().any(|e| e["stage"] == "Ready"
            || e["stage"] == "Go"
            || e["event"] == "frame_presented"));
        assert!(rows.iter().any(|e| e["event"] == "audicle_cancelled"));
        std::fs::remove_file(&host.trace).unwrap();
    }
    // These tests exercise the retained experimental shovel directly. The game
    // navigation route now uses PhysicsSystem, so give only these test hosts
    // the former caller permissions rather than changing production contracts.
    fn allow_experimental_shovel_test_calls(host: &mut Host) {
        let step = host.hooks.get_mut("PlanetNavigationSystem.Step").unwrap();
        for target in ["ShovelSystem.Step", "ShovelSystem.ConstrainMovement",
                       "TerrainToolSystem.Frame"] {
            step.calls.insert(target.into());
        }
        host.hooks.get_mut("PlanetNavigationSystem.Stop").unwrap()
            .calls.insert("ShovelSystem.Close".into());
    }

    #[test]
    fn soil_patch_exposes_visual_grass_separately_from_collision_mesh() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = std::env::temp_dir().join(format!("mirror-soil-grass-{}", std::process::id()));
        let mut host = Host::load_paths(root, &output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure", Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1, "ShovelSystem.Step".into()));
        let event = host.invoke("AudicleSystem.Create", Value::Null).unwrap();
        host.invoke("AudicleSystem.Ready", event.clone()).unwrap();
        host.invoke("AudicleSystem.Go", event).unwrap();
        let blade: Value = serde_json::from_slice(&fs::read(root.join("Procedures/Assets/shovel.json")).unwrap()).unwrap();
        let input = json!({"pose":{"position":[0.,6_371_000.04,0.],"right":[1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,1.]},"blade":blade["blade_collision"],"dt":0.,"equipped":true,"release":false});
        host.invoke("TerrainToolSystem.Prepare", input).unwrap();
        host.stack.pop();
        host.stack.push((2, "PlanetNavigationSystem.Step".into()));
        let frame = host.invoke("TerrainToolSystem.Frame", Value::Null).unwrap();
        assert_eq!(frame["grass"].as_array().unwrap().len(), 96);
        let visual=frame["visual"].as_array().unwrap().len();
        let physical=frame["debris"].as_array().unwrap().len();
        if frame["material"]["model"]=="material_preserving_polyhedral_cutting_with_affine_elasticity" {
            // One intact polyhedron has no internal interfaces to hide.
            assert_eq!(visual,physical);
        }else{
            assert!(visual<physical);
        }
        assert!(frame["debris"].as_array().unwrap().len() > 12);
        host.stack.pop();
        host.stack.push((3, "ShovelSystem.Step".into()));
        for (x,z) in [(0.,0.),(0.,0.15),(0.10,0.),(-0.10,0.)] {
            let hit=host.invoke("TerrainToolSystem.Aim",json!({"position":[x,6_371_001.,z],"forward":[0.,-1.,0.]})).unwrap();
            assert_eq!(hit["physical_soil"],true,"intact sod must cover material interfaces, not reveal the foundation: {hit}");
        }
        assert!(frame["material"]["skin_triangles"].as_u64().unwrap()>0);
        host.invoke("TerrainToolSystem.Release", Value::Null).unwrap();
        drop(host);
        let _ = fs::remove_dir_all(output);
    }

    #[cfg(target_os="macos")]
    #[test]
    fn failed_soil_contact_restores_previous_material_and_reports_failure() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-soil-contact-rollback-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        host.invoke("CaveSystem.Configure",Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1,"ShovelSystem.Step".into()));
        let event=host.invoke("AudicleSystem.Create",Value::Null).unwrap();
        host.invoke("AudicleSystem.Ready",event.clone()).unwrap();
        host.invoke("AudicleSystem.Go",event).unwrap();
        let blade:Value=serde_json::from_slice(&fs::read(root.join("Procedures/Assets/shovel.json")).unwrap()).unwrap();
        let mut input=json!({"pose":{"position":[0.,6_371_000.04,0.],"right":[1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,1.]},"blade":blade["blade_collision"],"dt":0.,"equipped":true,"release":false});
        let initial=host.invoke("TerrainToolSystem.Prepare",input.clone()).unwrap();
        host.stack.pop();host.stack.push((2,"TerrainToolSystem.Advance".into()));
        host.invoke("OGCConnectionSystem.Release",Value::Null).unwrap();
        host.stack.pop();host.stack.push((3,"ShovelSystem.Step".into()));
        input["dt"]=json!(1./60.);input["pose"]["position"]=json!([0.,6_371_000.03,0.]);
        let result=host.invoke("TerrainToolSystem.Advance",input).unwrap();
        assert!(result["failure"].as_str().is_some_and(|s|s.contains("not configured")));
        assert_eq!(result["material"]["mass"],initial["material"]["mass"]);
        assert_eq!(result["material"]["elements"],initial["material"]["elements"]);
        assert_eq!(result["contact"]["feedback_stale"],true);
        host.invoke("TerrainToolSystem.Release",Value::Null).unwrap();
        drop(host);let _=fs::remove_dir_all(output);
    }

    #[test]
    fn rooted_soil_exists_before_contact_and_detaches_through_load() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-rooted-contact-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure", Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1,"ShovelSystem.Step".into()));
        let event=host.invoke("AudicleSystem.Create",Value::Null).unwrap();
        host.invoke("AudicleSystem.Ready",event.clone()).unwrap();
        host.invoke("AudicleSystem.Go",event.clone()).unwrap();
        let blade:Value=serde_json::from_slice(&fs::read(root.join("Procedures/Assets/shovel.json")).unwrap()).unwrap();
        let mut input=json!({"pose":{"position":[0.,6_371_000.04,0.],"right":[1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,1.]},"blade":blade["blade_collision"],"dt":0.,"equipped":true,"release":false});
        let initial=host.invoke("TerrainToolSystem.Prepare",input.clone()).unwrap();
        assert!(initial["failure"].is_null(),"{}",initial);
        assert_eq!(initial["material"]["detached_elements"],0);
        let original_volume=initial["material"]["solid_volume"].as_f64().unwrap();
        assert!(original_volume>0.05,"the physical soil must fill a real domain before contact");
        let started=std::time::Instant::now();
        input["dt"]=json!(1./60.);
        for frame in 0usize..260 {
            assert!(started.elapsed().as_secs()<40,"bounded native contact check exceeded 40 seconds");
            let i=frame.saturating_sub(120).min(109); // Two seconds of idle soil, then stroke and final contact hold.
            let (height,angle)=if i<12 {(0.04,0.)} else if i<55 {(0.04-(i-12)as f64*0.006,0.)} else if i<85 {(-0.218+(i-55)as f64*0.003,(i-55)as f64*0.03)} else {(-0.128+(i-85)as f64*0.012,0.9)};
            input["pose"]["position"]=json!([0.,6_371_000.+height,0.]);
            input["pose"]["up"]=json!([0.,angle.cos(),angle.sin()]);
            input["pose"]["forward"]=json!([0.,-angle.sin(),angle.cos()]);
            let state=host.invoke("TerrainToolSystem.Advance",input.clone()).unwrap();
            assert!(state["failure"].is_null(),"{}",state);
            if frame==119 {assert_eq!(state["material"]["detached_elements"],0,"idle gravity must not automatically excavate soil: {}",state);}
            if frame%40==0 {let mut material=state["material"].clone();material.as_object_mut().unwrap().remove("parcels");eprintln!("rooted frame {i}: {} pose={} force={}",material,state["pose"],state["force"]);}
        }
        host.stack.pop();host.stack.push((2,"PlanetNavigationSystem.Step".into()));
        let soil=host.invoke("TerrainToolSystem.Frame",Value::Null).unwrap();
        eprintln!("rooted final {}",serde_json::to_string(&json!({"material":soil["material"],"steps":soil["accepted_steps"],"rejected":soil["rejected_steps"],"contacts":soil["peak_contact_pairs"],"timing":soil["timing"],"min_rigid_bound":soil["minimum_rigid_bound"],"near_zero_vertices":soil["near_zero_rigid_vertices"]})).unwrap());
        assert!(soil["simulated_seconds"].as_f64().unwrap()>1.5,"accepted simulation must advance rather than report stale forces from a stalled step");
        assert!(soil["peak_contact_pairs"].as_u64().unwrap()>0);
        assert!(soil["material"]["roots_broken"].as_u64().unwrap()>0,"levering must break real root constraints");
        assert!(soil["material"]["detached_elements"].as_u64().unwrap()>0,"material must separate under actual contact");
        assert!(soil["material"]["detached_elements"].as_u64().unwrap()<soil["material"]["elements"].as_u64().unwrap(),"an anchored remainder must stay intact");
        assert!((soil["rest_volume"].as_f64().unwrap()-original_volume).abs()<original_volume*0.001);
        assert!(soil["material"]["peak_lift"].as_f64().unwrap()>0.015,"detached soil must lift through contact");
        host.stack.pop();host.stack.push((3,"ShovelSystem.Step".into()));
        input["equipped"]=json!(false);input["release"]=json!(true);
        let released=host.invoke("TerrainToolSystem.Advance",input.clone()).unwrap();
        assert!(released["failure"].is_null());
        host.invoke("TerrainToolSystem.Release",Value::Null).unwrap();
        host.invoke("AudicleSystem.Finish",event).unwrap();
        if let Ok(path)=std::env::var("MIRROR_NATIVE_SOIL_VERIFICATION") {
            fs::write(path,serde_json::to_vec_pretty(&json!({"check":"native_rooted_soil_contact","passed":true,"initial_solid_volume_m3":original_volume,"frame":soil,"requested_frames":260,"idle_frames":120,"input_hz":60,"includes_final_contact_hold":true,"wall_seconds":started.elapsed().as_secs_f64(),"phone_playtest":false})).unwrap()).unwrap();
        }
        drop(host);let _=fs::remove_dir_all(output);

    }

    #[test]
    fn low_tool_stance_lowers_shoulders_and_camera_together() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-tool-stance-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        host.invoke("CaveSystem.Configure",Value::Null).unwrap();
        host.stack.push((1,"PlayerRigSystem.Prepare".into()));
        let model=host.invoke("PlayerModelSystem.Load",Value::Null).unwrap();
        host.invoke("InverseKinematicsSystem.Configure",model.clone()).unwrap();
        host.stack.clear();host.stack.push((2,"PlayerRigSystem.Advance".into()));
        let mut input=json!({"position":[0.,6_371_001.84,0.],"velocity":[0.,0.,0.],"forward":[0.,0.,-1.],"move":[false,false,false,false,false,false],"eye_height":1.84,"grounded":true,"flying":false,"dt":1./60.,"reset":false,"tool_grip":null,"tool_posture":{"bend_radians":0.,"forward":[0.,0.,-1.],"lowering_metres":0.}});
        let standing=host.invoke("InverseKinematicsSystem.Solve",input.clone()).unwrap();
        input["tool_posture"]["lowering_metres"]=json!(0.25);
        let mut low=Value::Null;
        for _ in 0..40 {low=host.invoke("InverseKinematicsSystem.Solve",input.clone()).unwrap();}
        for side in 0..2 {
            let a=standing["tool_reach"]["shoulders"][side][1].as_f64().unwrap();
            let b=low["tool_reach"]["shoulders"][side][1].as_f64().unwrap();
            assert!((a-b-0.25).abs()<1e-5);
        }
        assert!((standing["view_offset"][1].as_f64().unwrap()-low["view_offset"][1].as_f64().unwrap()-0.25).abs()<1e-5);
        let joints=model["joints"].as_array().unwrap();
        let index=|name:&str|joints.iter().position(|j|j["name"]==name).unwrap();
        for side in ["L","R"] {
            let ids=["Thigh","Shin","Foot"].map(|part|index(&format!("{part}.{side}")));
            for pair in ids.windows(2) {
                let length=|pose:&Value| {let a:[f64;3]=serde_json::from_value(pose["joints"][pair[0]].clone()).unwrap();let b:[f64;3]=serde_json::from_value(pose["joints"][pair[1]].clone()).unwrap();a.iter().zip(b).map(|(a,b)|(a-b).powi(2)).sum::<f64>().sqrt()};
                assert!((length(&standing)-length(&low)).abs()<1e-4,"working stance must preserve leg lengths");
            }
        }
        drop(host);let _=fs::remove_dir_all(output);
    }

    #[test]
    fn held_spade_limits_player_travel_and_release_removes_limit() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-spade-body-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure",Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1,"PlanetNavigationSystem.Step".into()));
        let mut input=json!({"entity":1,"position":[0.,6_371_001.84,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"dt":1./60.,"action":false,"reset":false,"touch":null,"tool_input":{"held":false,"rotate":false,"drop":false,"delta":[0.,0.],"twist":0.,"reach":0.}});
        host.invoke("ShovelSystem.Step",input.clone()).unwrap();
        input["tool_input"]["drop"]=json!(true);
        host.invoke("ShovelSystem.Step",input.clone()).unwrap();
        input["tool_input"]["drop"]=json!(false);
        for _ in 0..30 {host.invoke("ShovelSystem.Step",input.clone()).unwrap();}
        let request=json!({"previous":input["position"],"position":[0.,6_371_001.84,5.],"forward":input["forward"],"tool_reach":null});
        let held=host.invoke("ShovelSystem.ConstrainMovement",request.clone()).unwrap();
        assert!(held["fraction"].as_f64().unwrap()<0.2,"player cannot leave a held spade five metres behind: {held}");
        input["tool_input"]["drop"]=json!(true);
        host.invoke("ShovelSystem.Step",input).unwrap();
        assert_eq!(host.invoke("ShovelSystem.ConstrainMovement",request).unwrap()["fraction"],1.);
        host.stack.clear();host.stack.push((2,"PlanetNavigationSystem.Stop".into()));
        host.invoke("ShovelSystem.Close",Value::Null).unwrap();
        assert_eq!(host.entities.len(),0);
        drop(host);let _=fs::remove_dir_all(output);
    }

    #[test]
    fn guided_shovel_requests_physical_motion_and_manual_control_cancels_it() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = std::env::temp_dir().join(format!("mirror-guided-shovel-{}", std::process::id()));
        let mut host = Host::load_paths(root, &output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure", Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1, "PlanetNavigationSystem.Step".into()));
        let mut input = json!({"entity":1,"position":[0.,6_371_001.84,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"dt":1./60.,"action":false,"reset":false,"touch":null,"tool_input":{"held":false,"rotate":false,"drop":false,"delta":[0.,0.],"twist":0.,"reach":0.}});
        host.invoke("ShovelSystem.Step", input.clone()).unwrap();
        input["tool_input"]["drop"] = json!(true);
        assert_eq!(host.invoke("ShovelSystem.Step", input.clone()).unwrap()["equipped"], true);
        input["tool_input"]["drop"] = json!(false);
        input["forward"] = json!([0., -1.84, -0.60]);
        input["action"] = json!(true);
        let guided = host.invoke("ShovelSystem.Step", input.clone()).unwrap();
        assert!(host.shovel.guided_active());
        assert!(guided["physics"]["failure"].is_null(), "{guided}");
        input["action"] = json!(false);
        for i in 0..450 {
            let frame = host.invoke("ShovelSystem.Step", input.clone()).unwrap();
            assert!(frame["physics"]["failure"].is_null(), "{frame}");
            if i%60==0 {eprintln!("guided {i}: phase={} result={} pose={} material={}",frame["guided_phase"],frame["guided_result"],frame["tool_pose"],frame["physics"]["material"]);}
        }
        assert!(!host.shovel.guided_active(), "the guided motion should finish");
        input["action"] = json!(true);
        host.invoke("ShovelSystem.Step", input.clone()).unwrap();
        assert!(host.shovel.guided_active());
        input["action"] = json!(false);
        input["tool_input"]["held"] = json!(true);
        host.invoke("ShovelSystem.Step", input).unwrap();
        assert!(!host.shovel.guided_active());
        host.stack.clear();
        host.stack.push((2, "PlanetNavigationSystem.Stop".into()));
        host.invoke("ShovelSystem.Close", Value::Null).unwrap();
        drop(host);
        let _ = fs::remove_dir_all(output);
    }

    #[test]
    fn mobile_work_button_lifts_the_same_soil_with_reachable_hands() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-mobile-dig-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure",Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1,"PlayerRigSystem.Prepare".into()));
        let model=host.invoke("PlayerModelSystem.Load",Value::Null).unwrap();
        host.invoke("InverseKinematicsSystem.Configure",model).unwrap();
        host.stack.clear();host.stack.push((2,"PlanetNavigationSystem.Step".into()));
        let mut input=json!({"entity":1,"position":[0.,6_371_001.84,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"dt":1./60.,"action":false,"reset":false,"touch":null,"tool_input":{"held":false,"rotate":false,"drop":false,"delta":[0.,0.],"twist":0.,"reach":0.,"work":false}});
        host.invoke("ShovelSystem.Step",input.clone()).unwrap();
        input["tool_input"]["drop"]=json!(true);
        host.invoke("ShovelSystem.Step",input.clone()).unwrap();
        input["tool_input"]["drop"]=json!(false);
        let started=std::time::Instant::now();
        let mut state=Value::Null;
        let mut trace=Vec::new();
        let mut phase=Value::Null;
        for i in 0..720 {
            assert!(started.elapsed().as_secs()<90,"mobile digging fixture exceeded 90 seconds");
            input["tool_input"]["work"]=json!(i>=120);
            let camera=if input["camera_position"].is_null(){input["position"].clone()}else{input["camera_position"].clone()};
            // Aim the camera at the work site from the actual head/torso camera,
            // rather than retaining a direction computed from the neutral eye.
            let target=[0.,6_370_999.965,-0.60];
            input["aim"]=json!(std::array::from_fn::<_,3,_>(|a|target[a]-camera[a].as_f64().unwrap()));
            host.stack.clear();host.stack.push((2,"PlanetNavigationSystem.Step".into()));
            state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();
            assert!(state["physics"]["failure"].is_null(),"{state}");
            if i==0 || i==119 || state["guided_phase"]!=phase || i%60==0 {
                let mut material=state["physics"]["material"].clone();material["parcels"]=Value::Null;
                eprintln!("mobile dig {i}: phase={} result={} pose={} material={material}",state["guided_phase"],state["guided_result"],state["tool_pose"]);
                let frame=host.invoke("TerrainToolSystem.Frame",Value::Null).unwrap();
                trace.push(json!({"frame":i,"state":state,"soil":frame}));
                phase=state["guided_phase"].clone();
            }
            host.stack.clear();host.stack.push((3,"PlayerRigSystem.Advance".into()));
            let body=host.invoke("InverseKinematicsSystem.Solve",json!({"position":input["position"],"velocity":[0.,0.,0.],"forward":input["forward"],"move":[false,false,false,false,false,false],"eye_height":1.84,"grounded":true,"flying":false,"dt":1./60.,"reset":false,"tool_grip":state["tool_grip"],"tool_posture":state["tool_posture"]})).unwrap();
            for side in ["left","right"] {
                assert_eq!(body["tool_grip"][format!("{side}_reachable")],true,"hands must reach the contact-accepted spade: {}",body["tool_grip"]);
            }
            input["tool_reach"]=body["tool_reach"].clone();
            input["camera_position"]=json!(std::array::from_fn::<_,3,_>(|a|input["position"][a].as_f64().unwrap()+body["view_offset"][a].as_f64().unwrap()));
            if i>120 && !state["guided_result"].is_null() {break;}
        }
        if let Ok(path)=std::env::var("MIRROR_MOBILE_DIG_CAPTURE") {
            fs::write(path,serde_json::to_vec(&json!({"trace":trace,"final":state,"phone_playtested":false})).unwrap()).unwrap();
        }
        assert_eq!(state["guided_result"],"completed","the actual held mobile control must lift this stroke's earth; moving an empty shovel is not acceptance");
        host.stack.clear();host.stack.push((4,"PlanetNavigationSystem.Stop".into()));
        host.invoke("ShovelSystem.Close",Value::Null).unwrap();
        assert_eq!(host.entities.len(),0);
        drop(host);let _=fs::remove_dir_all(output);
    }

    #[test]
    fn player_spade_controls_reach_rooted_material_and_release_it() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-player-soil-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure", Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(true);
        host.stack.push((1,"PlayerRigSystem.Prepare".into()));
        let model=host.invoke("PlayerModelSystem.Load",Value::Null).unwrap();
        host.invoke("InverseKinematicsSystem.Configure",model).unwrap();
        host.stack.clear();host.stack.push((2,"PlanetNavigationSystem.Step".into()));

        let mut input=json!({"entity":1,"position":[0.,6_371_001.84,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"dt":1./60.,"action":false,"reset":false,"touch":null,"tool_input":{"held":false,"rotate":false,"drop":false,"delta":[0.,0.],"twist":0.,"reach":0.}});
        let cold=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(cold["equipped"],false);
        input["tool_input"]["drop"]=json!(true);
        let mut state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(state["equipped"],true);
        input["tool_input"]["drop"]=json!(false);
        let mut last_body=Value::Null;
        let mut hand_frames=0usize;let mut maximum_latched_hand_error=0f64;let mut maximum_acquisition_hand_error=0f64;
        let started=std::time::Instant::now();
        let mut i=0usize;
        let mut idle_left=0usize;
        let mut idle_verified=0usize;
        let mut initial_volume=None;
        while i<480 {
            let pausing=idle_left>0;

            assert!(started.elapsed().as_secs()<40,"bounded player soil route exceeded 40 seconds");
            // The soil now exists at a fixed work site before pickup. Lower the upright
            // blade into that site, use a short lever within its boundaries, then lift.
            input["position"][2]=json!(0.);
            input["tool_input"]["held"]=json!((20..45).contains(&i)||(144..194).contains(&i)||(304..374).contains(&i));
            input["tool_input"]["rotate"]=json!((254..284).contains(&i));
            input["tool_input"]["delta"]=json!([if (20..45).contains(&i){-4.}else{0.},if (144..194).contains(&i){4.4}else if (254..284).contains(&i){-5.}else if (304..374).contains(&i){-2.4}else{0.}]);
            input["tool_input"]["twist"]=json!(0.);
            input["tool_input"]["reach"]=json!(if (254..284).contains(&i) {let a=(i-254)as f64*0.03; -1.45*((a+0.03).sin()-a.sin())/0.002} else {0.});
            if pausing {input["tool_input"]=json!({"held":false,"rotate":false,"drop":false,"delta":[0.,0.],"twist":0.,"reach":0.});}
            state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();
            if initial_volume.is_none() && state["physics"]["material"]["initialized"]==true {
                let volume=state["physics"]["material"]["solid_volume"].as_f64().unwrap();
                assert!(volume>0.05,"a genuinely occupied material domain must precede contact");
                initial_volume=Some(volume);idle_left=120;
            } else if pausing {
                assert_eq!(state["physics"]["material"]["detached_elements"],0,"idle gravity must not excavate soil");
                assert_eq!(state["physics"]["material"]["roots_broken"],0,"untouched roots must survive idle settling");
                idle_left-=1;idle_verified+=1;
            }

            assert!(state["physics"]["failure"].is_null(),"{state}");
            host.stack.clear();host.stack.push((3,"PlayerRigSystem.Advance".into()));
            let body=host.invoke("InverseKinematicsSystem.Solve",json!({"position":input["position"],"velocity":[0.,0.,0.],"forward":input["forward"],"move":[false,false,false,false,false,false],"eye_height":1.84,"grounded":true,"flying":false,"dt":1./60.,"reset":false,"tool_grip":state["tool_grip"],"tool_posture":state["tool_posture"]})).unwrap();
            for side in ["left","right"] {
                assert_eq!(body["tool_grip"][format!("{side}_reachable")],true,"grip target out of anatomical reach at input frame {i}: feedback={}, reach={}, targets={}, posture={}",body["tool_grip"],body["tool_reach"],state["tool_grip"],state["tool_posture"]);
                let error=body["tool_grip"][format!("{side}_error")].as_f64().unwrap();
                if body["tool_grip"]["blend"].as_f64().unwrap()>=0.99 {
                    maximum_latched_hand_error=maximum_latched_hand_error.max(error);
                    assert!(error<0.05,"latched hand detached at input frame {i}: {}",body["tool_grip"]);
                } else { maximum_acquisition_hand_error=maximum_acquisition_hand_error.max(error); }
            }
            hand_frames+=1;
            last_body=body.clone();

            input["tool_reach"]=body["tool_reach"].clone();
            input["camera_position"]=json!(std::array::from_fn::<_,3,_>(|i|input["position"][i].as_f64().unwrap()+body["view_offset"][i].as_f64().unwrap()));
            host.stack.clear();host.stack.push((4,"PlanetNavigationSystem.Step".into()));

            if !pausing && i%50==0 {eprintln!("player soil frame {i}: pose={} request={} posture={} reach={} material={}",state["tool_pose"],state["physics"]["requested_pose"],state["tool_posture"],body["tool_reach"],state["physics"]["material"]);}
            if !pausing {i+=1;}
        }
        assert_eq!(idle_verified,120);
        let frame=host.invoke("TerrainToolSystem.Frame",Value::Null).unwrap();
        assert!(frame["simulated_seconds"].as_f64().unwrap()>1.5);
        assert!(frame["peak_contact_pairs"].as_u64().unwrap()>0);
        assert!(frame["material"]["detached_elements"].as_u64().unwrap()<frame["material"]["elements"].as_u64().unwrap(),"an anchored remainder must survive");
        let initial_volume=initial_volume.unwrap();
        assert!((frame["rest_volume"].as_f64().unwrap()-initial_volume).abs()<initial_volume*0.001);
        assert!(frame["budget_discarded_seconds"].as_f64().unwrap()<1e-8,"nominal input time must not disappear");
        for side in ["left","right"] {
            assert_eq!(last_body["tool_grip"][format!("{side}_reachable")],true,"accepted tool must remain physically reachable by the displayed hands: {}",last_body["tool_grip"]);
            assert!(last_body["tool_grip"][format!("{side}_error")].as_f64().unwrap()<0.05,"accepted tool hand error: {}",last_body["tool_grip"]);
        }
        let material=state["physics"]["material"].clone();

        assert!(material["roots_broken"].as_u64().unwrap()>0,"player controls must rupture roots: {material}");
        assert!(material["detached_elements"].as_u64().unwrap()>0,"player controls must separate existing soil: {material}");
        assert!(material["peak_lift"].as_f64().unwrap()>0.015,"player controls must lift the separated soil: {material}");
        input["tool_input"]["held"]=json!(false);input["tool_input"]["delta"]=json!([0.,0.]);input["tool_input"]["drop"]=json!(true);
        let released=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(released["equipped"],false);
        input["tool_input"]["drop"]=json!(false);
        for _ in 0..12 {state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert!(state["physics"]["failure"].is_null());}
        assert_eq!(state["physics"]["material"]["mass"],material["mass"]);
        host.stack.clear();host.stack.push((2,"PlanetNavigationSystem.Stop".into()));host.invoke("ShovelSystem.Close",Value::Null).unwrap();assert_eq!(host.entities.len(),0);
        if let Ok(path)=std::env::var("MIRROR_SOIL_VERIFICATION") {
            std::fs::write(path,serde_json::to_vec_pretty(&json!({"check":"production_player_spade_rooted_soil","passed":true,"material":material,"hand_feedback":last_body["tool_grip"],"hand_frames_checked":hand_frames,"maximum_latched_hand_error_m":maximum_latched_hand_error,"maximum_acquisition_hand_error_m":maximum_acquisition_hand_error,"native_frame":frame,"initial_solid_volume_m3":initial_volume,"idle_verified_frames":idle_verified,"release_material":state["physics"]["material"],"elapsed_seconds":started.elapsed().as_secs_f64(),"phone_playtest":false})).unwrap()).unwrap();
        }
        drop(host);let _=fs::remove_dir_all(output);
    }

    #[test]
    fn spade_pickup_grips_release_and_repickup_use_the_accepted_pose() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("mirror-spade-hooks-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        allow_experimental_shovel_test_calls(&mut host);
        host.invoke("CaveSystem.Configure", Value::Null).unwrap();
        host.terrain_tool.select_soil_experiment(false);
        host.stack.push((1,"PlayerRigSystem.Prepare".into()));
        let model=host.invoke("PlayerModelSystem.Load",Value::Null).unwrap();
        host.invoke("InverseKinematicsSystem.Configure",model).unwrap();
        host.stack.clear();host.stack.push((2,"PlanetNavigationSystem.Step".into()));
        let mut input=json!({"entity":1,"position":[0.,6_371_001.84,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.],"dt":1./60.,"action":false,"reset":false,"touch":null,"tool_input":{"held":false,"rotate":false,"drop":false,"delta":[0.,0.],"twist":0.,"reach":0.}});
        let cold=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(cold["equipped"],false);assert_eq!(cold["physics"]["active"],false);
        input["tool_input"]["drop"]=json!(true);
        let picked=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(picked["equipped"],true);assert!(picked["physics"]["failure"].is_null(),"{}",picked["physics"]);
        input["tool_input"]["drop"]=json!(false);
        let mut state=picked;
        let started=std::time::Instant::now();
        for _ in 0..45{assert!(started.elapsed().as_secs()<30);state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert!(state["physics"]["failure"].is_null(),"{}",state["physics"]);}
        for side in ["left","right"]{
            for axis in 0..3{
                let position=state["tool_pose"]["position"][axis].as_f64().unwrap();
                let expected=position+["right","up","forward"].iter().enumerate().map(|(j,k)|state["tool_pose"][*k][axis].as_f64().unwrap()*state["local_grips"][if side=="left"{0}else{1}][j].as_f64().unwrap()).sum::<f64>();
                assert!((state["tool_grip"][side][axis].as_f64().unwrap()-expected).abs()<1e-6);
            }
        }
        host.stack.clear();host.stack.push((3,"PlayerRigSystem.Advance".into()));
        let mut pose=Value::Null;
        for _ in 0..35{pose=host.invoke("InverseKinematicsSystem.Solve",json!({"position":input["position"],"velocity":[0.,0.,0.],"forward":input["forward"],"move":[false,false,false,false,false,false],"eye_height":1.84,"grounded":true,"flying":false,"dt":1./60.,"reset":false,"tool_grip":state["tool_grip"],"tool_posture":state["tool_posture"]})).unwrap();}
        eprintln!("spade carry actual IK feedback: {}",pose["tool_grip"]);
        assert_eq!(pose["tool_grip"]["left_reachable"],true);assert_eq!(pose["tool_grip"]["right_reachable"],true);
        assert!(pose["tool_grip"]["left_error"].as_f64().unwrap()<0.05);assert!(pose["tool_grip"]["right_error"].as_f64().unwrap()<0.05);
        host.stack.clear();host.stack.push((4,"PlanetNavigationSystem.Step".into()));
        // Check actual ground contact after rotating the shaft upright and lowering it.
        input["tool_input"]["rotate"]=json!(true);input["tool_input"]["delta"]=json!([0.,10.]);
        for _ in 0..9{state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();}
        input["tool_input"]["rotate"]=json!(false);input["tool_input"]["held"]=json!(true);input["tool_input"]["delta"]=json!([0.,12.]);
        let mut touched=false;
        for _ in 0..50{state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();let force=&state["physics"]["force"];touched|=force.as_array().unwrap().iter().any(|x|x.as_f64().unwrap().abs()>0.01);}
        eprintln!("spade ground contact: pose={}, force={}, touched={touched}",state["tool_pose"],state["physics"]["force"]);
        assert!(touched,"manual lowering must reach native ground contact");
        host.stack.clear();host.stack.push((6,"PlayerRigSystem.Advance".into()));
        for _ in 0..35{pose=host.invoke("InverseKinematicsSystem.Solve",json!({"position":input["position"],"velocity":[0.,0.,0.],"forward":input["forward"],"move":[false,false,false,false,false,false],"eye_height":1.84,"grounded":true,"flying":false,"dt":1./60.,"reset":false,"tool_grip":state["tool_grip"],"tool_posture":state["tool_posture"]})).unwrap();}
        eprintln!("spade ground actual IK feedback: {}",pose["tool_grip"]);
        assert_eq!(pose["tool_grip"]["left_reachable"],true);assert_eq!(pose["tool_grip"]["right_reachable"],true);
        assert!(pose["tool_grip"]["left_error"].as_f64().unwrap()<0.05);assert!(pose["tool_grip"]["right_error"].as_f64().unwrap()<0.05);
        host.stack.clear();host.stack.push((7,"PlanetNavigationSystem.Step".into()));
        input["tool_input"]["held"]=json!(true);input["tool_input"]["delta"]=json!([5.,8.]);
        state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(state["equipped"],true);
        input["tool_input"]["held"]=json!(false);input["tool_input"]["delta"]=json!([0.,0.]);input["tool_input"]["drop"]=json!(true);
        let released=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(released["equipped"],false);
        input["tool_input"]["drop"]=json!(false);
        for _ in 0..8{state=host.invoke("ShovelSystem.Step",input.clone()).unwrap();assert_eq!(state["equipped"],false);assert!(state["physics"]["failure"].is_null());}
        assert_eq!(host.entities.len(),1,"release keeps the physical tool entity alive");
        input["tool_input"]["drop"]=json!(true);state=host.invoke("ShovelSystem.Step",input).unwrap();assert_eq!(state["equipped"],true);
        host.stack.clear();host.stack.push((5,"PlanetNavigationSystem.Stop".into()));host.invoke("ShovelSystem.Close",Value::Null).unwrap();assert_eq!(host.entities.len(),0);drop(host);let _=fs::remove_dir_all(output);
    }

    #[test]
    fn computer_focus_preserves_game_texture_document_and_workstation_layout() {
        fn query(h:&mut Host,hook:&str)->Value {
            let stack=std::mem::take(&mut h.stack);
            let result=h.invoke(hook,Value::Null);
            h.stack=stack;
            result.unwrap()
        }
        let mut h=traced_host();
        h.backend="WebGPU".into();
        h.stack.push((800,"ApplicationSystem.Run".into()));
        h.invoke("AudioWorkstationSystem.Prewarm",Value::Null).unwrap();
        h.invoke("AudioWorkstationSystem.Prewarm",Value::Null).unwrap();
        assert_eq!(h.entities.len(),1,"prewarm owns one resident workstation Entity");
        assert_eq!(query(&mut h,"AudioWorkstationSystem.Status")["parked"],true);
        let resident=query(&mut h,"AudioWorkstationSystem.Status")["visual"].clone();
        assert!(h.active_audicle.is_none());
        assert!(h.graphics.prepared_entity().is_none());
        h.invoke("CueSystem.Begin",json!({"procedure":"planet_surface"})).unwrap();
        let game=h.invoke("CueSystem.Advance",Value::Null).unwrap()["visual"].clone();
        let owner=h.graphics.texture_owner();assert!(owner.is_some());
        let game_entities=h.entities.len();
        let prepared=h.graphics.prepared_entity();
        let document_before=query(&mut h,"WorkstationDataSystem.Snapshot");
        h.invoke("AudioWorkstationSystem.Background",Value::Null).unwrap();
        assert_eq!(h.graphics.prepared_entity(),prepared,"background keeps game input ownership");
        assert_eq!(query(&mut h,"WorkstationDataSystem.Snapshot"),document_before);
        assert_eq!(query(&mut h,"AudioWorkstationSystem.Status")["visual"],resident);
        h.invoke("PlanetNavigationSystem.Pause",Value::Null).unwrap();
        h.theatre.venue=Some(std::sync::Arc::new(crate::theatre::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()));
        h.invoke("AudioWorkstationSystem.PrimeWorld",Value::Null).unwrap();
        assert_eq!(h.graphics.prepared_entity(),prepared);
        h.invoke("GraphicsConnectionSystem.Suspend",game.clone()).unwrap();
        assert_eq!(h.graphics.suspended_entity(),game["entity"].as_u64());
        assert_eq!(h.graphics.suspended_texture_owner(),owner);
        assert!(h.invoke("GraphicsConnectionSystem.Suspend",game.clone()).is_err());
        h.stack.clear();
        let mut observer=h.environment.view().clone();
        observer["position"]=json!([0.5,8.6,-49.]);
        observer["forward"]=json!([0.,0.,1.]);observer["right"]=json!([-1.,0.,0.]);observer["up"]=json!([0.,1.,0.]);
        h.environment.set_camera_test_source(observer.clone());
        h.invoke("AudioWorkstationSystem.Resume",json!({"computer_screen":true})).unwrap();
        assert_eq!(h.audio_workstation.frozen_observer(),&observer,"computer focus never moves or rotates the observer");
        assert_eq!(h.environment.view(),&observer,"computer focus keeps the gameplay pose");
        assert_eq!(query(&mut h,"AudioWorkstationSystem.Status")["visual"],resident);
        h.invoke("AudioWorkstationSystem.SetPanel",json!({"edge":"bottom","expanded":true})).unwrap();
        workstation_bubble_open_fixture(&mut h);
        let step=json!({"dt":1.,"aspect":16./9.,"viewport":[1280.,720.],"framebuffer":[1280.,720.],"canvas_scale":1.,"keys":[false,false,false,false,false]});
        let before=h.invoke("AudioWorkstationSystem.Step",step).unwrap();
        let document=query(&mut h,"WorkstationDataSystem.Snapshot");
        h.invoke("AudioWorkstationSystem.Suspend",Value::Null).unwrap();
        assert_eq!(h.environment.view(),&observer,"releasing the computer retains the same pose");
        assert_eq!(h.entities.len(),game_entities);
        h.stack.push((801,"ApplicationSystem.Run".into()));
        h.invoke("GraphicsConnectionSystem.Resume",game.clone()).unwrap();
        assert_eq!(h.graphics.prepared_entity(),game["entity"].as_u64());
        assert_eq!(h.graphics.texture_owner(),owner);
        assert!(h.graphics.suspended_entity().is_none());
        h.invoke("GraphicsConnectionSystem.Suspend",game.clone()).unwrap();
        h.stack.clear();
        let reopened=h.invoke("AudioWorkstationSystem.Resume",Value::Null).unwrap();
        assert_eq!(reopened["panels"],before["panels"]);
        assert_eq!(reopened["piano_roll"],before["piano_roll"]);
        assert_eq!(query(&mut h,"WorkstationDataSystem.Snapshot"),document);
        h.invoke("AudioWorkstationSystem.Suspend",Value::Null).unwrap();
        h.stack.push((802,"ApplicationSystem.Run".into()));
        h.invoke("GraphicsConnectionSystem.Resume",game).unwrap();
        h.invoke("AudioWorkstationSystem.Close",Value::Null).unwrap();
        h.invoke("CueSystem.Close",Value::Null).unwrap();
        assert_eq!(h.entities.len(),0);
        assert!(h.graphics.prepared_entity().is_none());
        assert!(h.graphics.suspended_entity().is_none());
        assert!(h.active_audicle.is_none());
        let path=h.trace.clone();drop(h);let _=fs::remove_file(path);
    }

    #[test]
    fn failed_computer_presentation_can_restore_and_retry_retained_game() {
        let mut h=traced_host();
        h.stack.push((803,"ApplicationSystem.Run".into()));
        h.invoke("CueSystem.Begin",json!({"procedure":"planet_surface"})).unwrap();
        let game=h.invoke("CueSystem.Advance",Value::Null).unwrap()["visual"].clone();
        let owner=h.graphics.texture_owner();
        let game_entities=h.entities.len();
        h.invoke("GraphicsConnectionSystem.Suspend",game.clone()).unwrap();
        h.graphics.fail_prepare=true;
        h.stack.clear();
        let error=h.invoke("AudioWorkstationSystem.Resume",Value::Null).unwrap_err();
        assert!(error.contains("injected preparation failure"),"{error}");
        assert!(error.contains("cleanup: None"),"{error}");
        assert_eq!(h.entities.len(),game_entities);
        assert!(!h.entities.owned_by("AudioWorkstationSystem"));
        assert!(h.active_audicle.is_none());
        assert!(h.graphics.prepared_entity().is_none());
        assert_eq!(h.graphics.suspended_entity(),game["entity"].as_u64());
        h.stack.push((804,"ApplicationSystem.Run".into()));
        assert!(h.invoke("GraphicsConnectionSystem.Resume",game.clone()).is_err());
        assert_eq!(h.graphics.suspended_texture_owner(),owner);
        h.graphics.fail_prepare=false;
        h.invoke("GraphicsConnectionSystem.Resume",game.clone()).unwrap();
        assert_eq!(h.graphics.texture_owner(),owner);
        h.invoke("GraphicsConnectionSystem.Suspend",game.clone()).unwrap();
        h.stack.clear();
        assert_eq!(h.invoke("AudioWorkstationSystem.Resume",Value::Null).unwrap()["open"],true);
        h.invoke("AudioWorkstationSystem.Suspend",Value::Null).unwrap();
        assert_eq!(h.entities.len(),game_entities);
        assert!(h.active_audicle.is_none());
        h.stack.push((805,"ApplicationSystem.Run".into()));
        h.invoke("GraphicsConnectionSystem.Resume",game).unwrap();
        assert_eq!(h.graphics.texture_owner(),owner);
        h.invoke("CueSystem.Close",Value::Null).unwrap();
        assert_eq!(h.entities.len(),0);
        assert!(h.graphics.suspended_entity().is_none());
        let path=h.trace.clone();drop(h);let _=fs::remove_file(path);
    }

    #[cfg(target_os="macos")]
    #[test]
    fn workstation_preparation_cleanup_survives_plugin_hide_failure() {
        let mut h=traced_host();
        // Reject only the auxiliary plugin hide request during failed setup.
        // The original preparation error and the hide error must both survive.
        assert!(h.hooks.get_mut("AudioWorkstationSystem.Begin").unwrap()
            .calls.remove("PluginConnectionSystem.Command"));
        h.graphics.fail_prepare=true;
        let error=h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap_err();
        assert!(error.contains("injected preparation failure"),"{error}");
        assert!(error.contains("undeclared call AudioWorkstationSystem.Begin -> PluginConnectionSystem.Command"),"{error}");
        assert_eq!(h.entities.len(),0);
        assert!(h.active_audicle.is_none());
        assert!(h.graphics.prepared_entity().is_none());
        assert_eq!(h.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap()["open"],false);
        h.hooks.get_mut("AudioWorkstationSystem.Begin").unwrap()
            .calls.insert("PluginConnectionSystem.Command".into());
        h.graphics.fail_prepare=false;
        h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
        h.invoke("AudioWorkstationSystem.Suspend",Value::Null).unwrap();
        assert_eq!(h.entities.len(),0);
        assert!(h.active_audicle.is_none());
        let path=h.trace.clone();drop(h);let _=fs::remove_file(path);
    }

}

#[cfg(test)] mod shared_snapshot_route_tests {
 use super::*;
 #[test] fn shared_snapshot_keeps_registered_permissions_and_immutable_identity(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR"));let output=std::env::temp_dir().join(format!("wormwood-shared-snapshot-route-{}",std::process::id()));
  let mut host=Host::load_paths(root,&output).unwrap();let a=host.workstation_snapshot_shared().unwrap();
  let owned=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
  assert_eq!(a.document["tracks"],owned["tracks"]);assert_eq!(a.live["visualization"],owned["visualization"]);
  assert!(std::sync::Arc::ptr_eq(&a.document,&host.workstation_snapshot_shared().unwrap().document));
  host.enabled.insert("WorkstationDataSystem".into(),false);assert!(host.workstation_snapshot_shared().is_err());
  drop(host);let _=std::fs::remove_dir_all(output);
 }
}

include!("AudioPreparation.rs");

#[cfg(test)]
mod deferred_scene_pause_tests {
 use super::*;
 #[test]
 fn scene_publication_discards_finished_requesting_audicle(){
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
  let output=std::env::temp_dir().join(format!("mirror-scene-pause-{}",std::process::id()));
  let mut host=Host::load_paths(root,&output).unwrap();
  host.stack.push((1,"PlanetNavigationSystem.Step".into()));
  let id=host.invoke("AudicleSystem.Create",Value::Null).unwrap();
  host.invoke("AudicleSystem.Ready",id.clone()).unwrap();
  host.invoke("AudicleSystem.Go",id.clone()).unwrap();
  host.invoke("AudicleSystem.Finish",id.clone()).unwrap();
  host.stack.clear();
  crate::base_system::layer_four::appearance_system::check_finished_scene_discard(&mut host,id["audicle"].as_u64());
 }
}

#[cfg(test)]
#[path="GlassesTests.rs"]
mod glasses_tests;

#[cfg(test)]mod stage_cc_overlay_tests {
 use super::*;
 #[test]fn cc_curve_overlay_accepts_document_point_capacity_without_relaxing_audio_fades(){
  let points=vec![json!([100.,100.]);2048];let mut v=json!({"entity":1,"range":{"fades":[{"kind":"cc","clip":[0.,0.,200.,100.],"points":points,"handles":points}]}});
  assert!(check_payload("SelectionRangeRequest",&v));
  v["range"]["fades"][0]["kind"]=Value::Null;assert!(!check_payload("SelectionRangeRequest",&v));
  v["range"]["fades"][0]["kind"]=json!("cc");v["range"]["fades"][0]["handles"][0]=json!([f64::NAN,0.]);assert!(!check_payload("SelectionRangeRequest",&v));
 }
}

#[cfg(test)]
#[path="CraftingTests.rs"]
mod crafting_tests;

#[cfg(test)]
mod waterpark_configuration_tests {
    use super::*;
    use crate::audio::{add,sub,length,V};
    #[test]
    fn waterpark_production_configuration_installs_world_space_pools_and_contacts() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));
        let out=std::env::temp_dir().join(format!("wormwood-waterpark-host-{}",std::process::id()));
        let mut host=Host::load_paths(root,&out).unwrap();
        // Configure/Close are normally invoked by navigation. This test grants
        // those test root calls; their nested calls retain real contracts.
        host.roots.extend(["WaterSystem.Configure".into(),"WaterSystem.Close".into(),"TheatreSystem.ResolveBody".into()]);
        host.invoke("TheatreSystem.Configure",Value::Null).unwrap();
        host.invoke("WaterSystem.Configure",Value::Null).unwrap();
        let park=host.theatre.venue.as_ref().unwrap().park.as_ref().unwrap();
        assert!(host.water.hydrology.bodies.pools.len()>=3);
        assert!(!host.water.hydrology.bodies.sheets.is_empty());
        let pool=&host.water.hydrology.bodies.pools[1];
        assert_eq!(pool.min,add(park.pools[1].0,host.theatre.origin));
        let position=host.theatre.world(add(park.offset,[0.,2.,30.]));
        let result=host.invoke("TheatreSystem.ResolveBody",json!({"previous":position,"position":add(position,[0.,-0.1,0.]),"eye_height":2.})).unwrap();
        let accepted:V=serde_json::from_value(result["position"].clone()).unwrap();
        assert!(length(sub(accepted,position))<0.02);
        // The new golf course reaches beyond the old 900-metre collision cutoff.
        let venue=host.theatre.venue.as_ref().unwrap();
        let hit=venue.trace_local([850.,100.,-360.],[0.,-1.,0.],200.).unwrap();
        let standing=host.theatre.world(add(hit.position,[0.,1.7,0.]));
        let body=host.invoke("TheatreSystem.ResolveBody",json!({"previous":standing,"position":add(standing,[0.,-0.08,0.]),"eye_height":1.7})).unwrap();
        let accepted:V=serde_json::from_value(body["position"].clone()).unwrap();
        assert!((accepted[1]-standing[1]).abs()<0.03,"far golf ground support missing");
        host.invoke("WaterSystem.Close",Value::Null).unwrap();
    }
}

#[cfg(all(test,unix))]
mod laboratory_integration_tests {
    use super::*;
    use crate::audio::{add,sub,length};
    #[test]
    fn laboratory_gift_persists_portals_traverse_and_owned_entities_close() {
        let source=Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture=std::env::temp_dir().join(format!("wormwood-laboratory-contracts-{}",std::process::id()));
        let _=fs::remove_dir_all(&fixture);
        for folder in ["BaseSystem","Systems","Procedures","output"]{fs::create_dir_all(fixture.join(folder)).unwrap();}
        let names=["LaboratorySystem","EntitySystem","AudicleSystem","TheatreSystem","PlanetNavigationSystem"];
        let mut registry=serde_json::Map::new();let mut catalog=read(source,"Procedures/functionregistry.json").unwrap();
        catalog["systems"].as_object_mut().unwrap().retain(|name,_|names.contains(&name.as_str()));
        for name in names {
            registry.insert(name.into(),json!(true));let mut contract=read(source,&format!("Systems/{name}.json")).unwrap();
            if name=="PlanetNavigationSystem" {
                catalog["systems"][name]["functions"].as_object_mut().unwrap().retain(|hook,_|["Step","Stop"].contains(&hook.as_str()));
                contract=json!({"system":name,"layer":5,"functions":{"Step":{"calls":["LaboratorySystem.Step","LaboratorySystem.Snapshot"]},"Stop":{"calls":["LaboratorySystem.Close"]}},"init_steps":[]});
            }
            fs::write(fixture.join(format!("Systems/{name}.json")),serde_json::to_vec(&contract).unwrap()).unwrap();
        }
        fs::write(fixture.join("BaseSystem/registry.json"),serde_json::to_vec(&registry).unwrap()).unwrap();
        fs::write(fixture.join("BaseSystem/circuit_breaker.json"),br#"{"halt_execution":false}"#).unwrap();
        fs::write(fixture.join("Procedures/functionregistry.json"),serde_json::to_vec(&catalog).unwrap()).unwrap();
        fs::write(fixture.join("Procedures/systems.json"),serde_json::to_vec(&json!({"systems_order":names.map(|n|format!("{n}.json"))})).unwrap()).unwrap();
        for entry in fs::read_dir(source.join("Procedures")).unwrap() {
            let entry=entry.unwrap();let name=entry.file_name();if name!="functionregistry.json"&&name!="systems.json" {std::os::unix::fs::symlink(entry.path(),fixture.join("Procedures").join(name)).unwrap();}
        }
        let mut host=Host::load_paths(&fixture,&fixture.join("output")).unwrap();host.graphics.test_driver=true;
        // The workspace's older Nightworld asset has a stale waterpark datum.
        // Construct the real town venue here without loading that independent
        // asset; all laboratory, Entity and Audicle hooks remain contracted.
        let origin=crate::theatre::waterpark::theatre_origin();
        let n=crate::audio::unit([0.,crate::terrain::RADIUS,-94.]);
        let outside=(n[1]*(crate::terrain::RADIUS+crate::terrain::height(n))-origin[1]).min(0.);
        let mut venue=crate::theatre::load(source).unwrap();venue.add_approach(outside).unwrap();
        venue.add_waterpark(source,crate::theatre::waterpark::site_offset(origin),outside,origin).unwrap();venue.add_town(source,origin).unwrap();
        let civic=venue.add_civic_buildings(source,origin,outside).unwrap();venue.add_marsh(source,origin,civic).unwrap();venue.add_plan(source,origin,civic).unwrap();venue.add_laboratory(source,origin).unwrap();
        host.theatre.origin=origin;host.theatre.venue=Some(std::sync::Arc::new(venue));
        let site=host.theatre.venue.as_ref().unwrap().laboratory.clone().unwrap();
        let before=host.entities.len();let events=host.audicles.count();
        host.stack.push((1400,"PlanetNavigationSystem.Step".into()));
        let mut request=json!({"position":origin,"dt":0.,"eye_height":2.,"talk":false});
        let initial=host.invoke("LaboratorySystem.Step",request.clone()).unwrap();assert_eq!(initial["stage"],"Waiting");assert_eq!(host.entities.len(),before+2);
        request["position"]=json!(add(origin,add(site.lab,[0.,2.,-6.])));
        let start=host.invoke("LaboratorySystem.Step",request.clone()).unwrap();assert_eq!(start["stage"],"Introduction");assert_eq!(start["locked"],true);
        for _ in 0..100 {assert_eq!(host.invoke("LaboratorySystem.Step",request.clone()).unwrap()["stage"],"Introduction");}
        request["dt"]=json!(0.25);let mut stages=std::collections::BTreeSet::new();
        for _ in 0..128 {let s=host.invoke("LaboratorySystem.Step",request.clone()).unwrap();stages.insert(s["stage"].as_str().unwrap().to_owned());}
        for stage in ["Gift","Follow","Portal","Farewell","Zoo"]{assert!(stages.contains(stage),"missing {stage}");}
        let done=host.invoke("LaboratorySystem.Step",request.clone()).unwrap();assert_eq!(done["stage"],"Zoo");assert_eq!(done["owned"],true);assert_eq!(done["locked"],false);
        assert!(fixture.join("output/Saves/laboratory.json").is_file());
        // Clear endpoint hysteresis, then cross the aperture with the whole body.
        request["position"]=json!(add(origin,add(site.lab_portal,[0.,0.2,-4.])));host.invoke("LaboratorySystem.Step",request.clone()).unwrap();
        request["position"]=json!(add(origin,add(site.lab_portal,[0.,0.2,-1.])));host.invoke("LaboratorySystem.Step",request.clone()).unwrap();
        request["position"]=json!(add(origin,add(site.lab_portal,[0.,0.2,1.])));
        let crossed=host.invoke("LaboratorySystem.Step",request.clone()).unwrap();let destination:crate::audio::V=serde_json::from_value(crossed["destination"].clone()).unwrap();
        assert!(length(sub(destination,add(origin,site.zoo_portal)))<3.);
        request["position"]=json!(destination);assert!(host.invoke("LaboratorySystem.Step",request.clone()).unwrap()["destination"].is_null());
        host.stack.clear();host.stack.push((1401,"PlanetNavigationSystem.Stop".into()));
        host.invoke("LaboratorySystem.Close",Value::Null).unwrap();host.invoke("LaboratorySystem.Close",Value::Null).unwrap();
        assert_eq!(host.entities.len(),before);assert_eq!(host.audicles.count(),events);assert!(host.active_audicle.is_none());
        host.stack.clear();host.stack.push((1402,"PlanetNavigationSystem.Step".into()));
        let restored=host.invoke("LaboratorySystem.Step",request).unwrap();assert_eq!(restored["stage"],"Zoo");assert_eq!(restored["owned"],true);
        host.stack.clear();host.stack.push((1403,"PlanetNavigationSystem.Stop".into()));host.invoke("LaboratorySystem.Close",Value::Null).unwrap();
        drop(host);fs::remove_dir_all(fixture).unwrap();
    }
}


