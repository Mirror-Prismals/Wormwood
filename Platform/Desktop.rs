//! Process entry outside System scope: parse, bootstrap in declared order, dispatch.
use crate::host::Host;
use std::path::Path;
enum Command<'a> {
    Diagnostics,
    Interactive(&'a str, &'a str, bool),
    Live(&'a str, &'a str, Option<u32>),
    Cue(&'a str, &'a str, u32),
    Triangle(&'a str, u32),
    AudioVisual(&'a str, u32),
    Soak(&'a str, u32, u32),
}
impl<'a> Command<'a> {
    fn parse(args: &'a [String]) -> Result<Self, String> {
        let command=match args{
   []=>Self::Diagnostics,
   [flag,name,backend] if flag=="--interactive"=>Self::Interactive(name,backend,false),
   [flag,name,backend,verify] if flag=="--interactive" && verify=="--verify-session"=>Self::Interactive(name,backend,true),
   [flag,name,backend] if flag=="--live"=>Self::Live(name,backend,None),
   [flag,name,backend,close,n] if flag=="--live" && close=="--close-after-frames"=>Self::Live(name,backend,Some(n.parse().map_err(|_|"invalid diagnostic close count")?)),
   [flag,name,backend] if flag=="--cue"=>Self::Cue(name,backend,120),
   [flag,name,backend,limit,frames] if flag=="--cue"&&limit=="--frames"=>Self::Cue(name,backend,frames.parse().map_err(|_|"invalid frame count")?),
   [flag,backend] if flag=="--triangle"=>Self::Triangle(backend,120),
   [flag,backend,limit,frames] if flag=="--triangle"&&limit=="--frames"=>Self::Triangle(backend,frames.parse().map_err(|_|"invalid frame count")?),
   [flag,backend] if flag=="--av"=>Self::AudioVisual(backend,120),
   [flag,backend,limit,frames] if flag=="--av"&&limit=="--frames"=>Self::AudioVisual(backend,frames.parse().map_err(|_|"invalid frame count")?),
   [flag,backend,duration,seconds,repeat,repeats] if flag=="--soak"&&duration=="--seconds"&&repeat=="--repeat"=>Self::Soak(backend,seconds.parse().map_err(|_|"invalid soak seconds")?,repeats.parse().map_err(|_|"invalid repeat count")?),
   _=>return Err("usage: wormwood --interactive NAME BACKEND [--verify-session] | --live NAME BACKEND | [--cue NAME BACKEND|--triangle BACKEND|--av BACKEND] [--frames 1..600] | --soak BACKEND --seconds 1..10 --repeat 1..20".into()),
  };
        let backend = match &command {
            Self::Diagnostics => None,
            Self::Interactive(_, b, _)
            | Self::Live(_, b, _)
            | Self::Cue(_, b, _)
            | Self::Triangle(b, _)
            | Self::AudioVisual(b, _)
            | Self::Soak(b, _, _) => Some(*b),
        };
        if backend.is_some_and(|b| !matches!(b, "OpenGL" | "Vulkan" | "WebGPU")) {
            return Err("backend must be OpenGL, Vulkan, or WebGPU".into());
        }
        match &command {
            Self::Interactive(name, _, _) | Self::Live(name, _, _) | Self::Cue(name, _, _)
                if !crate::base_system::layer_two::cue_procedure_system::valid_name(name) =>
            {
                return Err("invalid cue name".into());
            }
            Self::Cue(_, _, n) | Self::Triangle(_, n) | Self::AudioVisual(_, n)
                if !(1..=600).contains(n) =>
            {
                return Err("frame limit must be 1..600".into());
            }
            Self::Live(_, _, Some(n)) if !(1..=600).contains(n) => {
                return Err("diagnostic close count must be 1..600".into());
            }
            Self::Soak(_, seconds, repeats)
                if !(1..=10).contains(seconds) || !(1..=20).contains(repeats) =>
            {
                return Err("soak requires seconds 1..10 and repeats 1..20".into());
            }
            _ => {}
        }
        Ok(command)
    }
}
pub(super) fn run(args: &[String]) -> Result<(), String> {
    let command = Command::parse(args)?;
    let root = std::env::var_os("WORMWOOD_ASSET_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf());
    let output = std::env::var_os("WORMWOOD_OUTPUT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("output"));
    let mut host = Host::load_paths(&root, &output)?;
    let startup = host.startup()?;
    match command {
        Command::Diagnostics => {
            println!("{startup}");
            Ok(())
        }
        Command::Interactive(name, backend, verify) => host.run_interactive(name, backend, verify),
        Command::Live(name, backend, close) => host.run_live(name, backend, close),
        Command::Cue(name, backend, frames) => host.run_procedure(name, backend, frames),
        Command::Triangle(backend, frames) => host.run_triangle(backend, frames),
        Command::AudioVisual(backend, frames) => host.run_av(backend, frames),
        Command::Soak(backend, seconds, repeats) => host.run_soak(backend, seconds, repeats),
    }
}
