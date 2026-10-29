//! Shared policy: desktop and XR are projections of the same window identities.
pub mod scene;
pub mod session_state;
pub mod service;
pub use scene::*;

/// Geometry is opt-in from a JSON file; preview edits are session-local until exported.
pub fn load_geometry(scene: &mut Scene) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("DOME_CONFIG") {
        let config: Config = serde_json::from_slice(&std::fs::read(path)?)?;
        config.validate()?;
        scene.config = config;
    }
    Ok(())
}

pub mod shortcuts;
pub mod xr_input;

pub mod xr_transport;

pub mod desktop_entries;

pub mod taskbar;
pub mod workspace_switcher;

pub mod appearance;
pub mod keyboard;

pub mod xr_presentation;

pub mod input_routing;

pub mod context_menu;
pub mod geometry_settings;
pub mod gpu_settings;
pub mod spatial_popup;
pub mod terminal;
#[cfg(feature = "xr")]
pub mod xr_environment;

pub mod desktop_snap;

pub mod launch_context;
pub mod file_manager;
pub mod stereo;

pub mod audio;


pub mod music_floor;
pub mod performance_floor;

pub mod bundled_fonts;
pub mod font_picker;
pub mod visual_feedback;

pub mod xr_gpu;

#[cfg(feature = "xr")]
pub mod webxr_bridge;

pub mod askpass;

pub mod clock;

pub mod font_export;

pub mod screenshot;

pub mod super_mmb;

pub mod quick_launch;

pub mod xr_window_size;
pub mod wivrn;

pub mod audio_mixer;

pub mod native_xr;
pub mod webxr_focus;
pub mod window_visibility;

pub mod xr_backdrop;

pub mod appearance_panel;

pub mod desktop_backdrop;

pub mod system_sounds;
