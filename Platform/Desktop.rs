//! Process entry outside System scope: parse, bootstrap in declared order, dispatch.
use crate::host::Host;
use std::path::Path;
enum Command<'a> {
    Diagnostics,
    Workstation(&'a str, Option<u32>),
    BubbleNotesCheck(&'a str),
    AudioSpheresCheck(&'a str),
    PhoneCheck(&'a str),
    ArrangementCheck(&'a str),
    WorkstationControlsCheck(&'a str),
    WorkstationBenchmark(&'a str, u32),
    GameWorkstationBenchmark(&'a str, u32),
    Simulation(&'a str, u32),
    Workshop(&'a str, &'a str, Option<u32>),
    Interactive(&'a str, &'a str, bool),
    BoundedInteractive(&'a str, &'a str, bool, u32),
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
   [flag,backend] if flag=="--check-bubble-notes"=>Self::BubbleNotesCheck(backend),
   [flag,backend] if flag=="--check-audio-spheres"=>Self::AudioSpheresCheck(backend),
   [flag,backend] if flag=="--check-phone"=>Self::PhoneCheck(backend),
   [flag,backend] if flag=="--check-arrangement"=>Self::ArrangementCheck(backend),
   [flag,backend] if flag=="--check-workstation-controls"=>Self::WorkstationControlsCheck(backend),
   [flag,backend] if flag=="--benchmark-game-workstation"=>Self::GameWorkstationBenchmark(backend,32),
   [flag,backend,count,n] if flag=="--benchmark-game-workstation" && count=="--frames"=>Self::GameWorkstationBenchmark(backend,n.parse().map_err(|_|"invalid benchmark frame count")?),
   [flag,backend] if flag=="--benchmark-workstation"=>Self::WorkstationBenchmark(backend,32),
   [flag,backend,count,n] if flag=="--benchmark-workstation" && count=="--frames"=>Self::WorkstationBenchmark(backend,n.parse().map_err(|_|"invalid benchmark frame count")?),
   [flag,backend] if (flag=="--workstation" || flag=="--daw-only")=>Self::Workstation(backend,None),
   [flag,backend,count,n] if (flag=="--workstation" || flag=="--daw-only") && count=="--frames"=>Self::Workstation(backend,Some(n.parse().map_err(|_|"invalid workstation frame count")?)),
   [flag,name,backend] if flag=="--workshop"=>Self::Workshop(name,backend,None),
   [flag,name,backend,count,n] if flag=="--workshop" && count=="--frames"=>Self::Workshop(name,backend,Some(n.parse().map_err(|_|"invalid workshop frame count")?)),
   [flag,name,count,n] if flag=="--simulate" && count=="--frames"=>Self::Simulation(name,n.parse().map_err(|_|"invalid simulation frame count")?),
   [flag,name,backend] if flag=="--interactive"=>Self::Interactive(name,backend,false),
   [flag,name,backend,verify] if flag=="--interactive" && verify=="--verify-session"=>Self::Interactive(name,backend,true),
   [flag,name,backend,close,n] if flag=="--interactive" && close=="--close-after-frames"=>Self::BoundedInteractive(name,backend,false,n.parse().map_err(|_|"invalid diagnostic close count")?),
   [flag,name,backend,verify,close,n] if flag=="--interactive" && verify=="--verify-session" && close=="--close-after-frames"=>Self::BoundedInteractive(name,backend,true,n.parse().map_err(|_|"invalid diagnostic close count")?),
   [flag,name,backend] if flag=="--live"=>Self::Live(name,backend,None),
   [flag,name,backend,close,n] if flag=="--live" && close=="--close-after-frames"=>Self::Live(name,backend,Some(n.parse().map_err(|_|"invalid diagnostic close count")?)),
   [flag,name,backend] if flag=="--cue"=>Self::Cue(name,backend,120),
   [flag,name,backend,limit,frames] if flag=="--cue"&&limit=="--frames"=>Self::Cue(name,backend,frames.parse().map_err(|_|"invalid frame count")?),
   [flag,backend] if flag=="--triangle"=>Self::Triangle(backend,120),
   [flag,backend,limit,frames] if flag=="--triangle"&&limit=="--frames"=>Self::Triangle(backend,frames.parse().map_err(|_|"invalid frame count")?),
   [flag,backend] if flag=="--av"=>Self::AudioVisual(backend,120),
   [flag,backend,limit,frames] if flag=="--av"&&limit=="--frames"=>Self::AudioVisual(backend,frames.parse().map_err(|_|"invalid frame count")?),
   [flag,backend,duration,seconds,repeat,repeats] if flag=="--soak"&&duration=="--seconds"&&repeat=="--repeat"=>Self::Soak(backend,seconds.parse().map_err(|_|"invalid soak seconds")?,repeats.parse().map_err(|_|"invalid repeat count")?),
   _=>return Err("usage: wormwood [--performance] --daw-only BACKEND [--frames 1..3600] [--style PATH | --theme NAME_OR_PATH] (alias --workstation) | --check-workstation-controls BACKEND | --check-style PATH | --benchmark-workstation BACKEND [--frames 8..600] | --benchmark-game-workstation BACKEND [--frames 8..600] | --inspect-intent [--json] | --interactive NAME BACKEND [--verify-session] | --live NAME BACKEND | [--cue NAME BACKEND|--triangle BACKEND|--av BACKEND] [--frames 1..600] | --soak BACKEND --seconds 1..10 --repeat 1..20".into()),
  };
        let backend = match &command {
            Self::Diagnostics | Self::Simulation(_, _) => None,
            Self::Workstation(b, _)
            | Self::BubbleNotesCheck(b)
            | Self::AudioSpheresCheck(b)
            | Self::PhoneCheck(b)
            | Self::ArrangementCheck(b)
            | Self::WorkstationControlsCheck(b)
            | Self::WorkstationBenchmark(b, _)
            | Self::GameWorkstationBenchmark(b, _)
            | Self::Workshop(_, b, _)
            | Self::Interactive(_, b, _)
            | Self::BoundedInteractive(_, b, _, _)
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
            Self::WorkstationBenchmark(_, n) | Self::GameWorkstationBenchmark(_, n) if !(8..=600).contains(n) => {
                return Err("benchmark measured frame count must be 8..600".into());
            }
            Self::Workstation(_, Some(n)) if !(1..=3600).contains(n) => {
                return Err("workstation frame limit must be 1..3600".into());
            }
            Self::Interactive(name, _, _)
            | Self::BoundedInteractive(name, _, _, _)
            | Self::Live(name, _, _)
            | Self::Cue(name, _, _)
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
            Self::Workshop(name, _, n)
                if !crate::physics::is_case(name)
                    || n.is_some_and(|n| !(1..=3600).contains(&n)) =>
            {
                return Err("invalid workshop case/frame count".into());
            }
            Self::Simulation(name, n)
                if !crate::physics::is_case(name) || !(1..=3600).contains(n) =>
            {
                return Err("unknown scenario or frame count outside 1..3600".into());
            }
            _ => {}
        }
        Ok(command)
    }
}
pub(super) fn run(args: &[String]) -> Result<(), String> {
    let performance = args.iter().any(|s| s == "--performance");
    let filtered: Vec<_> = args
        .iter()
        .filter(|s| s.as_str() != "--performance")
        .cloned()
        .collect();
    let mut filtered = filtered;
    let mut style_path = None;
    if let Some(at) = filtered.iter().position(|s|s=="--style") {
        let path = filtered.get(at+1).cloned().ok_or("--style requires a package folder or style.json")?;
        filtered.drain(at..at+2);
        if filtered.iter().any(|s|s=="--style") { return Err("--style may be supplied only once".into()); }
        style_path = Some(path);
    }
    let mut theme_name = None;
    if let Some(at) = filtered.iter().position(|s|s=="--theme") {
        let name=filtered.get(at+1).cloned().ok_or("--theme requires ruby-3d, cardinal, a preset name, or a JSON file")?;
        filtered.drain(at..at+2);
        if filtered.iter().any(|s|s=="--theme"){return Err("--theme may be supplied only once".into());}
        theme_name=Some(name);
    }
    let mut selected_mods=Vec::new();let mut mod_paths=Vec::new();
    while let Some(at)=filtered.iter().position(|s|s=="--mod"||s=="--mod-dir") {
        let value=filtered.get(at+1).cloned().ok_or("--mod and --mod-dir require a value")?;
        if filtered[at]=="--mod"{selected_mods.push(value);}else{mod_paths.push(std::path::PathBuf::from(value));}
        filtered.drain(at..at+2);
    }
    // CLI setup precedes Host construction and every worker thread.
    if !selected_mods.is_empty(){unsafe{std::env::set_var("MIRROR_MODS",selected_mods.join(","));}}
    if !mod_paths.is_empty(){let paths=std::env::join_paths(mod_paths).map_err(|e|e.to_string())?;unsafe{std::env::set_var("MIRROR_MOD_PATH",paths);}}
    let args = filtered.as_slice();
    let root = std::env::var_os("WORMWOOD_ASSET_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf());
    let mod_diagnostics=std::env::var_os("WORMWOOD_OUTPUT_ROOT").map(std::path::PathBuf::from).unwrap_or_else(||root.join("output/mod-inspection"));
    if let [flag,hook]=args {if flag=="--inspect-mod-system" {
        let mut host=crate::host::Host::load_paths(&root,&mod_diagnostics)?;
        println!("{}",host.invoke(hook,serde_json::Value::Null)?);return Ok(());
    }}
    if args==["--inspect-world"] {
        let mut host=crate::host::Host::load_paths(&root,&mod_diagnostics)?;
        host.invoke("TheatreSystem.Configure",serde_json::Value::Null)?;
        println!("{}",serde_json::json!({"mods":host.mods.inspect()?,"theatre":serde_json::json!({"ready":host.theatre.venue.is_some(),"origin":host.theatre.origin,"computer":host.theatre.computer_position()}),"town":host.mods.provides("town"),"waterpark":host.theatre.venue.as_ref().is_some_and(|v|v.park.is_some()),"nightworld":host.theatre.nightworld.is_some(),"vertices":host.theatre.venue.as_ref().map_or(0,|v|v.vertices.len())}));return Ok(());
    }
    if args==["--list-mods"]||args==["--check-mods"] {
        let mut mods=crate::modding::Manager::from_root(&root)?;
        println!("{}",serde_json::to_string_pretty(&mods.inspect()?).map_err(|e|e.to_string())?);return Ok(());
    }
    #[cfg(not(target_os="ios"))]
    if let [flag,backend]=args {
        if flag=="--check-trees" {
            let out=std::env::var_os("WORMWOOD_OUTPUT_ROOT").map(std::path::PathBuf::from).unwrap_or_else(||root.join("output/checks/trees-20261108").join(backend));
            return crate::base_system::layer_one::graphics_connection_system::check_trees(&root,&out,backend);
        }
        if flag=="--check-flashlight" {
            let out=std::env::var_os("WORMWOOD_OUTPUT_ROOT").map(std::path::PathBuf::from).unwrap_or_else(||root.join("output/checks/flashlight-20261108/native"));
            return crate::base_system::layer_one::graphics_connection_system::check_flashlight(&root,&out,backend);
        }
        if flag=="--check-theatre-exterior" {
            let out=std::env::var_os("WORMWOOD_OUTPUT_ROOT").map(std::path::PathBuf::from).unwrap_or_else(||root.join("output/checks/exterior-limestone/native"));
            return crate::base_system::layer_one::graphics_connection_system::check_theatre_exterior(&root,&out,backend);
        }
    }
    if args.first().is_some_and(|s| s == "--inspect-intent") {
        let as_json = match args {
            [flag] if flag == "--inspect-intent" => false,
            [flag, format] if flag == "--inspect-intent" && format == "--json" => true,
            _ => return Err("usage: wormwood --inspect-intent [--json]".into()),
        };
        let report = crate::host::intent::inspect(&root)?;
        if as_json {
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
            );
        } else {
            print!("{}", crate::host::intent::plain(&report));
        }
        return Ok(());
    }
    if let [flag,path] = args {
        if flag=="--check-style" {
            if style_path.is_some() { return Err("--check-style takes its path directly".into()); }
            let kit=crate::workstation::user_style::Kit::load(Path::new(path))?;
            println!("{}",serde_json::json!({"id":kit.id,"name":kit.name,"validated":true,
                "animated":kit.animated,"background":kit.background,"device_rendering":"not_checked"}));
            return Ok(());
        }
    }
    if args==["--list-themes"] {
        println!("{}",serde_json::json!({"main":"ruby-3d","themes":crate::workstation::cardinal_theme::presets(&root)}));return Ok(());
    }
    if let [flag,from,to]=args {
        if flag=="--copy-theme" {crate::workstation::cardinal_theme::Theme::load(Path::new(from))?.save_copy(Path::new(to))?;return Ok(());}
    }
    if let [flag,path]=args {
        if flag=="--check-theme" {
            let theme=crate::workstation::cardinal_theme::Theme::load(Path::new(path))?;
            println!("{}",theme.summary());return Ok(());
        }
    }
    if let Some(name)=theme_name.as_deref(){crate::workstation::cardinal_theme::select(&root,name)?;}
    if crate::workstation::cardinal_theme::selected(&root)?.is_some() {
        if style_path.is_some()||crate::workstation::user_style::selected()?.is_some(){return Err("select either a Cardinal 2D theme or a custom model style".into());}
        if std::env::var("WORMWOOD_WORKSTATION_REFERENCE_OPTICS").as_deref()==Ok("1"){return Err("Cardinal 2D themes require the production compositor".into());}
    }
    let command = Command::parse(args)?;
    if let Some(path) = style_path {
        if !matches!(command,Command::Workstation(..)|Command::WorkstationBenchmark(..)) {
            return Err("--style is available with --workstation, --daw-only, or --benchmark-workstation".into());
        }
        crate::workstation::user_style::select_path(Path::new(&path))?;
    }
    crate::host::performance_telemetry::set_enabled(
        performance
            || (matches!(command, Command::WorkstationBenchmark(..) | Command::GameWorkstationBenchmark(..))
                && std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_PROFILE").as_deref() != Ok("0"))
            || std::env::var("WORMWOOD_PERFORMANCE").as_deref() == Ok("1"),
    );
    let output = std::env::var_os("WORMWOOD_OUTPUT_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("output"));
    let mut host = Host::load_paths(&root, &output)?;
    let startup = host.startup()?;
    let result = match command {
        Command::Diagnostics => {
            println!("{startup}");
            Ok(())
        }
        Command::Workshop(case, backend, frames) => host.run_workshop(case, backend, frames),
        Command::Workstation(backend, frames) => host.run_workstation(backend, frames),
        Command::BubbleNotesCheck(backend) => host.check_bubble_notes(backend),
        Command::AudioSpheresCheck(backend) => host.check_audio_spheres(backend),
        Command::PhoneCheck(backend) => host.check_phone(backend),
        Command::ArrangementCheck(backend) => host.check_arrangement(backend),
        Command::WorkstationControlsCheck(backend) => host.check_workstation_controls(backend),
        Command::WorkstationBenchmark(backend, frames) => {
            host.run_workstation_benchmark(backend, frames)
        }
        Command::GameWorkstationBenchmark(backend, frames) => host.run_game_workstation_benchmark(backend, frames),
        Command::Simulation(case, frames) => host.run_simulation(case, frames),
        Command::Interactive(name, backend, verify) => host.run_interactive(name, backend, verify),
        Command::BoundedInteractive(name, backend, verify, frames) => {
            host.run_interactive_bounded(name, backend, verify, frames)
        }
        Command::Live(name, backend, close) => host.run_live(name, backend, close),
        Command::Cue(name, backend, frames) => host.run_procedure(name, backend, frames),
        Command::Triangle(backend, frames) => host.run_triangle(backend, frames),
        Command::AudioVisual(backend, frames) => host.run_av(backend, frames),
        Command::Soak(backend, seconds, repeats) => host.run_soak(backend, seconds, repeats),
    };
    let performance_result = host.write_performance_report();
    result.and(performance_result)
}

#[cfg(test)] mod combined_benchmark_tests {
 use super::*;
 fn args(s:&[&str])->Vec<String>{s.iter().map(|s|s.to_string()).collect()}
 #[test] fn explicit_bounded_combined_entry(){
  assert!(matches!(Command::parse(&args(&["--benchmark-game-workstation","WebGPU"])).unwrap(),Command::GameWorkstationBenchmark("WebGPU",32)));
  for n in ["0","7","601","not-a-count"] {assert!(Command::parse(&args(&["--benchmark-game-workstation","WebGPU","--frames",n])).is_err());}
  assert!(Command::parse(&args(&["--benchmark-game-workstation","unknown"])).is_err());
 }
 #[test] fn ordinary_entries_remain_distinct(){
  assert!(matches!(Command::parse(&args(&["--daw-only","WebGPU"])).unwrap(),Command::Workstation(..)));
  assert!(matches!(Command::parse(&args(&["--interactive","planet_surface","WebGPU"])).unwrap(),Command::Interactive(..)));
 }
}
