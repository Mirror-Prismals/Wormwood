//! Serialized control/preparation document operations for the seconds-based workstation.
//! Free-time clips remain independent; optional warped audio uses the shared
//! Cardinal musical-time map.
//! No function in this module belongs in a real-time audio callback.
#[path = "Automation.rs"]
mod automation;
#[cfg(test)]
#[path = "BackendTests.rs"]
mod backend_tests;
#[path = "Buses.rs"]
pub(crate) mod buses;
#[path = "Document.rs"]
mod document;
#[path="Channels.rs"]
mod channels;
#[path = "Guides.rs"]
mod guides;
#[path = "Panel.rs"]
mod panel;
#[path = "Playback.rs"]
mod playback;
#[path = "MixPreview.rs"]
pub(crate) mod mix_preview;
#[cfg(test)]
#[path = "PlaybackTests.rs"]
mod playback_tests;
#[path = "Rhythm.rs"]
mod rhythm;
#[path = "Routing.rs"]
mod routing;
#[path = "TrackIo.rs"]
pub(crate) mod track_io;
#[path = "KeyboardToggle.rs"]
pub(crate) mod keyboard_toggle;
#[path = "TrackIoScene.rs"]
pub(crate) mod track_io_scene;
#[path = "TrackInputBlock.rs"]
mod track_input_block;
#[path = "AudioPlaylistMenu.rs"]
pub(crate) mod audio_playlist_menu;
#[path = "AudioPlaylists.rs"]
mod audio_playlists;
#[path = "Recording.rs"]
mod recording;
#[path = "MidiCapture.rs"]
mod midi_capture;

pub(crate) use document::{State, SharedSnapshot, dispatch};
#[path="SharedDocument.rs"] mod shared_document;
pub(crate) use shared_document::Owner;

#[path = "Export.rs"]
mod export;
#[path = "ExportWave.rs"]
mod export_wave;
#[path = "ExportDialog.rs"]
pub(crate) mod export_dialog;
#[cfg(test)]
#[path = "ExportTests.rs"]
mod export_tests;

#[path = "GemMesh.rs"]
pub(crate) mod gem_mesh;
#[path = "PanelMotion.rs"]
pub(crate) mod panel_motion;

#[path = "PanelLayout.rs"]
pub(crate) mod panel_layout;

#[path = "FloorLight.rs"]
pub(crate) mod floor_light;
#[path = "RenderDefaults.rs"]
pub(crate) mod render_defaults;
#[path = "DeferredFloor.rs"]
pub(crate) mod deferred_floor;

#[path = "OrigamiControl.rs"]
pub(crate) mod origami_control;

#[path = "TransportControls.rs"]
pub(crate) mod transport_controls;
#[path = "TransportPose.rs"]
pub(crate) mod transport_pose;

#[path = "DecorativeControls.rs"]
pub(crate) mod decorative_controls;

#[path = "ControlEngraving.rs"]
pub(crate) mod control_engraving;

#[path = "InputMonitor.rs"]
pub(crate) mod input_monitor;

#[path = "EdisonDemo.rs"]
pub(crate) mod edison_demo;

#[path = "Engraving.rs"]
pub(crate) mod engraving;

#[path = "FrameCadence.rs"]
pub(crate) mod frame_cadence;
#[path = "Benchmark.rs"]
pub(crate) mod benchmark;

#[path = "PianoRoll.rs"]
pub(crate) mod piano_roll;
#[path = "PianoRollScene.rs"]
pub(crate) mod piano_roll_scene;
#[path = "BubbleNotes.rs"]
pub(crate) mod bubble_notes;

#[path = "AudioSpheres.rs"]
pub(crate) mod audio_spheres;
#[path = "SourceAnalysis.rs"]
mod source_analysis;
#[cfg(test)]
#[path = "SourceAnalysisTests.rs"]
mod source_analysis_tests;
#[path = "SpatialMix.rs"]
pub(crate) mod spatial_mix;
#[path = "SphereDiagnostic.rs"]
pub(crate) mod sphere_diagnostic;

#[path = "Arrangement.rs"]
pub(crate) mod arrangement;
#[path = "Browser.rs"]
pub(crate) mod browser;
#[path = "TimelineScene.rs"]
pub(crate) mod timeline_scene;

#[path = "PluginRender.rs"]
mod plugin_render;

#[path = "PluginInserts.rs"]
pub(crate) mod plugin_inserts;

#[path = "CardinalTheme.rs"]
pub(crate) mod cardinal_theme;

#[path = "UserStyle.rs"]
pub(crate) mod user_style;
#[path = "StyleShader.rs"]
pub(crate) mod style_shader;
#[path = "ShaderSpecialization.rs"]
pub(crate) mod shader_specialization;

#[path = "RubyResponsePlan.rs"]
pub(crate) mod ruby_response_plan;
#[path = "RubyResolveReuse.rs"]
pub(crate) mod ruby_resolve_reuse;
#[path = "RubyResponseShader.rs"]
pub(crate) mod ruby_response_shader;

#[path = "TimelineBars.rs"]
pub(crate) mod timeline_bars;
#[path = "PluginBrowser.rs"]
pub(crate) mod plugin_browser;
#[path = "PluginSurface.rs"]
pub(crate) mod plugin_surface;
#[path = "StockEq.rs"]
pub(crate) mod stock_eq;
#[path = "BottomPanel.rs"]
pub(crate) mod bottom_panel;
#[path = "LoopbackHistory.rs"]
pub(crate) mod loopback_history;
#[path = "SpectralRepair.rs"]
pub(crate) mod spectral_repair;
#[path = "ClipTreatment.rs"]
mod clip_treatment;

#[path = "BottomPanelAdapter.rs"]
pub(crate) mod bottom_panel_adapter;

#[path = "RetrospectiveRecall.rs"]
pub(crate) mod retrospective_recall;

#[cfg(test)]
#[path = "BottomPanelBackendTests.rs"]
mod bottom_panel_backend_tests;

#[path = "BottomPanelRuntime.rs"]
pub(crate) mod bottom_panel_runtime;

#[path = "WarpDsp.rs"]
pub(crate) mod warp_dsp;
#[path = "WarpMap.rs"]
pub(crate) mod warp_map;

#[path = "WarpTreatment.rs"]
mod warp_treatment;
pub(crate) use warp_treatment::Clock as WarpClock;

#[path="MixerMeters.rs"]
mod mixer_meters;

#[path="LoopControl.rs"] pub(crate) mod loop_control;
#[path="TimelineBarScene.rs"] pub(crate) mod timeline_bar_scene;

#[path="MusicalTime.rs"] pub(crate) mod musical_time;

#[path="RenderClock.rs"] pub(crate) mod render_clock;

#[path = "StockEqGems.rs"]
pub(crate) mod stock_eq_gems;


#[path="WarpTiming.rs"] mod warp_timing;
#[cfg(test)] #[path="WarpTimingTests.rs"] mod warp_timing_tests;

#[path="BottomPanelScene.rs"] pub(crate) mod bottom_panel_scene;

#[path="FaderComponents.rs"] pub(crate) mod fader_components;

#[cfg(test)]
#[path = "MidiCaptureIntegrationTests.rs"]
mod midi_capture_integration_tests;

#[path="CaptureTimeline.rs"] pub(crate) mod capture_timeline;


#[path="MidiKeyboard.rs"] pub(crate) mod midi_keyboard;
#[path = "CcAutomation.rs"]
pub(crate) mod cc_automation;
#[path = "CcParameterPicker.rs"]
pub(crate) mod cc_parameter_picker;



#[path="RetrospectiveBuffer.rs"] pub(crate) mod retrospective_buffer;
#[path="RetrospectiveInsert.rs"] mod retrospective_insert;
#[path="RetrospectiveRuntime.rs"] pub(crate) mod retrospective_runtime;

#[path="LivePanelData.rs"] pub(crate) mod live_panel_data;

#[path="BottomOverlay.rs"] pub(crate) mod bottom_overlay;
#[path="SourceTransients.rs"] mod source_transients;

#[path = "Lightbox.rs"]
pub(crate) mod lightbox;
#[path = "Settings.rs"]
pub(crate) mod settings;

#[path="CameraBackdrop.rs"]
pub(crate) mod camera_backdrop;

#[cfg(test)]
#[path = "SignalIntegrityTests.rs"]
mod signal_integrity_tests;

#[path="MobileWorkstationInput.rs"]
pub(crate) mod mobile_workstation_input;

#[path = "RainbowWaveform.rs"]
pub(crate) mod rainbow_waveform;

#[path="PianoWorkspace.rs"] pub(crate) mod piano_workspace;

#[path="DrumRack.rs"] pub(crate) mod drum_rack;
#[path="DrumRackUi.rs"] pub(crate) mod drum_rack_ui;
#[path="DrumSample.rs"] mod drum_sample;

#[path="PianoTuning.rs"] pub(crate) mod piano_tuning;
#[path="PianoScene.rs"] pub(crate) mod piano_scene;

#[path="PianoKeyLayout.rs"] pub(crate) mod piano_key_layout;

#[path = "AudioLibrary.rs"]
pub(crate) mod audio_library;

#[path="PausedWorld.rs"] pub(crate) mod paused_world;

#[path = "Tutorial.rs"]
pub(crate) mod tutorial;

#[path="ClipFades.rs"] pub(crate) mod clip_fades;

#[path="TrackFreeze.rs"] pub(crate) mod track_freeze;

#[path = "Undo.rs"]
mod undo;

#[path="StageLighting.rs"]
pub(crate) mod stage_lighting;

#[path="NoteBend.rs"] pub(crate) mod note_bend;

#[path="ComputerScreen.rs"]
pub(crate) mod computer_screen;

#[path="TimelineControls.rs"]
pub(crate) mod timeline_controls;

#[path="Viewports.rs"] pub(crate) mod viewports;

#[path="AudioPreview.rs"] pub(crate) mod audio_preview;

#[path = "AudioImport.rs"]
pub(crate) mod audio_import;

#[path = "PluginImport.rs"]
mod plugin_import;
