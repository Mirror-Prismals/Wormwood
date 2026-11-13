//! Layer-five experiment lifecycle. Visual and headless modes use the same material System.
use crate::host::Host;
use serde_json::{Value, json};
#[path = "WorkshopPerformance.rs"]
mod performance;
use performance::{Clock, Performance};
pub(crate) fn benchmark(host: &mut Host, input: &Value) -> Result<Value, String> {
    let event = host.invoke("AudicleSystem.Create", Value::Null)?;
    let run = (|| {
        host.invoke("PhysicsSystem.Configure", input.clone())?;
        host.invoke("AudicleSystem.Ready", event.clone())?;
        host.invoke("AudicleSystem.Go", event.clone())?;
        let mut report = Value::Null;
        let mut performance = Performance::default();
        let mut simulated = 0.;
        for _ in 0..input["frames"].as_u64().unwrap_or(60) {
            let started = std::time::Instant::now();
            report = host.invoke("PhysicsSystem.Advance", Value::Null)?;
            let elapsed = started.elapsed().as_secs_f64();
            let now = report["simulated_seconds"].as_f64().unwrap_or(simulated);
            performance.record(elapsed, elapsed, 0., 0., now - simulated, true);
            simulated = now;
            if report["halted"] == true {
                break;
            }
        }
        report["performance"] = performance.report();
        Ok::<_, String>(report)
    })();
    let cleanup = host.invoke("PhysicsSystem.Release", Value::Null);
    let end = if run.as_ref().is_ok_and(|v| v["halted"] == false) && cleanup.is_ok() {
        "AudicleSystem.Finish"
    } else {
        "AudicleSystem.Cancel"
    };
    let lifecycle = host.invoke(end, event);
    match (run, cleanup, lifecycle) {
        (Ok(report), Ok(_), Ok(_)) => Ok(
            json!({"mode":"simulation_workshop","physics":report,"entities_remaining":host.entities.len()}),
        ),
        (run, clean, end) => Err(format!(
            "workshop failed: {:?}; cleanup {:?}; lifecycle {:?}",
            run.err(),
            clean.err(),
            end.err()
        )),
    }
}

pub(crate) fn run(host: &mut Host, input: &Value) -> Result<Value, String> {
    let event = host.invoke("AudicleSystem.Create", Value::Null)?;
    let mut entity = Value::Null;
    let mut ready = false;
    let execution = (|| -> Result<Value, String> {
        let mut report = host.invoke("PhysicsSystem.Configure", input.clone())?;
        let mesh = host.invoke("PhysicsSystem.Mesh", Value::Null)?;
        entity = host.invoke("EntitySystem.Create", mesh)?;
        host.invoke("AppearanceSystem.Prepare", entity.clone())?;
        host.invoke("AudicleSystem.Ready", event.clone())?;
        host.invoke("AudicleSystem.Go", event.clone())?;
        ready = true;
        let soil_showcase = input["case"] == "soil_spade_fracture";
        let material_showcase = input["case"]
            .as_str()
            .is_some_and(|s| s.starts_with("matter_"));
        let worker_camera=matches!(input["case"].as_str(),Some("matter_worker_dig"|"matter_worker_axe"|"matter_snow"));
        let close_camera = soil_showcase || material_showcase;
        let initial_pos = if input["case"]=="matter_worker_axe" { [3.4f64,2.8,5.2] } else if worker_camera { [2.5f64,2.3,3.8] } else if input["case"]
            .as_str()
            .is_some_and(|s| s.starts_with("matter_grass"))
        {
            [0.82f64, 0.68, 1.18]
        } else if input["case"] == "matter_layers" {
            [1.12f64, 1.16, 1.62]
        } else if close_camera {
            [1.05f64, 1.05, 1.5]
        } else {
            [0f64, 3.2, 6.]
        };
        let initial_pitch = if worker_camera { -0.25 } else if close_camera { -0.36 } else { -0.25 };
        let initial_yaw = if close_camera { -0.61 } else { 0. };
        let mut pos = initial_pos;
        let mut yaw = initial_yaw;
        let mut pitch = initial_pitch;
        let mut previous = [false; 3];
        let mut paused = false;
        let mut frames = 0u64;
        let mut last = std::time::Instant::now();
        let mut scheduler_last = last;
        let mut clock = Clock::new();
        let mut performance = Performance::default();
        let mut caption_last = last - std::time::Duration::from_secs(1);
        let fixed_workload = input["frames"].as_u64().is_some();
        loop {
            let frame_start = std::time::Instant::now();
            let elapsed = frame_start.duration_since(scheduler_last).as_secs_f64();
            scheduler_last = frame_start;
            let mut physics_seconds = 0.;
            let mut mesh_seconds = 0.;
            let mut simulated_advance = 0.;
            let render_start = std::time::Instant::now();
            host.invoke("AppearanceSystem.View", json!({"entity":entity["entity"],"position":pos,"yaw":yaw,"pitch":pitch,"capture":input["frames"].as_u64().is_some_and(|n|frames+1==n)}))?;
            let mut render_seconds = render_start.elapsed().as_secs_f64();
            if caption_last.elapsed().as_secs_f64() >= 0.25 {
                caption_last = std::time::Instant::now();
                let caption = if material_showcase {
                    let detail = if report["worker"].is_object() {
                        let w=&report["worker"]; let m=&report["muscles"];
                        let mut line=format!("Stroke {} · effort {:.0}% · hands {:.0} N · contact {:.0} N · work {:.1} J · carried {}",
                            w["stroke"].as_u64().unwrap_or(1),m["activation"].as_f64().unwrap_or(0.)*100.,m["hand_force_n"].as_f64().unwrap_or(0.),
                            report["tool"]["contact_force_n"].as_f64().unwrap_or(0.),report["tool"]["contact_work_j"].as_f64().unwrap_or(0.),w["peak_carried_samples"].as_u64().unwrap_or(0));
                        if let Some(t)=report["thermal"].as_object() {
                            line+=&format!("\nAir {:.1}°C · heat time ×{:.0} · melted {:.2} kg",t["air_c"].as_f64().unwrap_or(0.),t["thermal_time_scale"].as_f64().unwrap_or(1.),t["melted_kg"].as_f64().unwrap_or(0.));
                        }
                        line
                    } else if report["thermal"].is_object() {
                        let t=&report["thermal"];
                        format!("Air {:.1}°C · water {:.1}°C · heat time ×{:.0} · melted {:.2} kg\nIce pieces {} · fractures {} · contact work {:.1} J",
                            t["air_c"].as_f64().unwrap_or(0.),t["water_c"].as_f64().unwrap_or(0.),t["thermal_time_scale"].as_f64().unwrap_or(1.),t["melted_kg"].as_f64().unwrap_or(0.),
                            report["solid"]["components"].as_u64().unwrap_or(0),report["solid"]["broken_interfaces"].as_u64().unwrap_or(0),report["tool"]["contact_work_j"].as_f64().unwrap_or(0.))
                    } else if report["layers"].is_object() {

                        format!(
                            "Torn anchoring roots {} · soil deposited {} · bedrock {} · stone pieces {}",
                            report["layers"]["torn_anchoring_roots"]
                                .as_u64()
                                .unwrap_or(0),
                            report["layers"]["soil_deposited_samples"]
                                .as_u64()
                                .unwrap_or(0),
                            if report["layers"]["bedrock_exposed"] == true {
                                "exposed"
                            } else {
                                "covered"
                            },
                            report["layers"]["rock"]["components"].as_u64().unwrap_or(0)
                        )
                    } else if report["sand"].is_object() {
                        format!(
                            "Fine sand · yielding parcels {} · peak bucket load {} parcels · contact work {:.2} J",
                            report["sand"]["yielding_points"].as_u64().unwrap_or(0),
                            report["peak_bucket_samples"].as_u64().unwrap_or(0),
                            report["tool"]["contact_work_j"].as_f64().unwrap_or(0.)
                        )
                    } else if report["turf"].is_object() {
                        format!(
                            "Turf crowns {} · severed roots {} · pieces {} · lifted patches {}",
                            report["turf"]["crowns"].as_u64().unwrap_or(0),
                            report["turf"]["severed_roots"].as_u64().unwrap_or(0),
                            report["turf"]["pieces"].as_u64().unwrap_or(0),
                            report["turf"]["lifted_elements"].as_u64().unwrap_or(0)
                        )
                    } else if matches!(
                        report["case"].as_str(),
                        Some("matter_wood" | "matter_wood_chop")
                    ) {
                        format!(
                            "Wood pieces {} · grain splits {} · cross-grain cuts {} · contact work {:.2} J",
                            report["solid"]["components"].as_u64().unwrap_or(0),
                            report["solid"]["grain_splits"].as_u64().unwrap_or(0),
                            report["solid"]["cross_grain_cuts"].as_u64().unwrap_or(0),
                            report["solid"]["tool_contact_work_j"]
                                .as_f64()
                                .unwrap_or(0.)
                        )
                    } else if report["solid"].is_object() {
                        format!(
                            "Solid pieces {} · broken interfaces {} · pressure {:.2} MPa · contact work {:.2} J",
                            report["solid"]["components"].as_u64().unwrap_or(0),
                            report["fractures"].as_u64().unwrap_or(0),
                            report["solid"]["peak_contact_pressure_pa"]
                                .as_f64()
                                .unwrap_or(0.)
                                / 1e6,
                            report["solid"]["tool_contact_work_j"]
                                .as_f64()
                                .unwrap_or(0.)
                        )
                    } else {
                        format!(
                            "Matter {} · local refinements {} · simplified {} · fractures {}",
                            report["active_samples"].as_u64().unwrap_or(0),
                            report["refinements"].as_u64().unwrap_or(0),
                            report["coarsenings"].as_u64().unwrap_or(0),
                            report["fractures"].as_u64().unwrap_or(0)
                        )
                    };
                    let trial = if report["comparison"].is_object() {
                        let c = &report["comparison"];
                        if c["complete"] == true {
                            c["results"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|r| {
                                    let outcome = if let Some(pieces) = r["solid_pieces"].as_u64() {
                                        format!("{pieces} pieces")
                                    } else if report["case"] == "matter_sand"
                                        || report["case"] == "matter_pebbles"
                                    {
                                        format!(
                                            "{} held",
                                            r["peak_bucket_samples"].as_u64().unwrap_or(0)
                                        )
                                    } else {
                                        format!(
                                            "{} breaks / {} lifted",
                                            r["fractures"].as_u64().unwrap_or(0),
                                            r["peak_lifted_samples"].as_u64().unwrap_or(0)
                                        )
                                    };
                                    format!("{}: {outcome}", r["force"].as_str().unwrap_or(""))
                                })
                                .collect::<Vec<_>>()
                                .join(" · ")
                        } else {
                            format!(
                                "Trial {}/3 · {} · drive limit {:.0} N · contact peak {:.0} N · work {:.2} J",
                                c["trial"].as_u64().unwrap_or(1),
                                c["label"].as_str().unwrap_or(""),
                                c["drive_limit_n"].as_f64().unwrap_or(0.),
                                report["tool"]["peak_contact_force_n"]
                                    .as_f64()
                                    .unwrap_or(0.),
                                report["tool"]["contact_work_j"].as_f64().unwrap_or(0.)
                            )
                        }
                    } else {
                        String::new()
                    };
                    format!(
                        "{} · {} · {}\n{detail}{}",
                        report["title"].as_str().unwrap_or("Material workshop"),
                        if report["halted"] == true {
                            "Stopped — reset"
                        } else if paused {
                            "Paused"
                        } else {
                            "Automatic"
                        },
                        report["phase"].as_str().unwrap_or("Preparing"),
                        if trial.is_empty() {
                            String::new()
                        } else {
                            format!("\n{trial}")
                        }
                    )
                } else if soil_showcase {
                    format!(
                        "Automatic soil fracture · {} · {}\nBroken bonds {} · detached cells {} · lifted cells {}",
                        if report["halted"] == true {
                            "HALTED — reset"
                        } else if paused {
                            "PAUSED"
                        } else {
                            "RUNNING"
                        },
                        report["soil_probe"]["phase"]
                            .as_str()
                            .unwrap_or("Preparing"),
                        report["soil_probe"]["broken_interfaces"]
                            .as_u64()
                            .unwrap_or(0),
                        report["soil_probe"]["detached_cells"].as_u64().unwrap_or(0),
                        report["soil_probe"]["lifted_cells"].as_u64().unwrap_or(0)
                    )
                } else {
                    format!(
                        "Mirror Physics Workshop | {} | {} | t={:.2}s | {:.1}ms/step | contacts {} | P pause · N step · R reset · Esc close",
                        input["case"].as_str().unwrap(),
                        if report["halted"] == true {
                            "HALTED — R to reset"
                        } else if paused {
                            "PAUSED"
                        } else {
                            "RUNNING"
                        },
                        report["simulated_seconds"].as_f64().unwrap_or(0.),
                        report["step_ms_average"].as_f64().unwrap_or(0.),
                        report["peak_contact_pairs"].as_u64().unwrap_or(0)
                    )
                };
                host.invoke(
                    "GraphicsConnectionSystem.Caption",
                    // Desktop window titles have one visible line. Keep the
                    // live counters first there; UIKit displays all lines.
                    json!({"entity":entity["entity"],"text":format!("{}\n{caption}", performance.caption(host.backend()))}),
                )?;
            }
            let pump_start = std::time::Instant::now();
            let window = host.invoke("AppearanceSystem.Pump", entity.clone())?;
            render_seconds += pump_start.elapsed().as_secs_f64();
            frames += 1;
            if window["closed"] == true
                || (!cfg!(target_os = "ios") && window["keys"][1] == true)
                || input["frames"].as_u64().is_some_and(|n| frames >= n)
            {
                performance.record(
                    frame_start.elapsed().as_secs_f64(),
                    0.,
                    0.,
                    render_seconds,
                    0.,
                    !paused && report["halted"] != true,
                );
                break;
            }
            if window["suspended"] == true {
                last = std::time::Instant::now();
                scheduler_last = last;
                clock.reset();
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            #[cfg(target_os = "ios")]
            let mobile_actions = crate::ios_touch::take_workshop_actions();
            #[cfg(not(target_os = "ios"))]
            let mobile_actions = 0u32;
            let keys = [
                !cfg!(target_os = "ios") && window["flight"]["pause"] == true,
                window["flight"]["step"] == true,
                window["flight"]["reset"] == true,
            ];
            if (keys[0] && !previous[0]) || mobile_actions & 1 != 0 {
                paused = !paused;
                clock.reset();
            }
            let mut changed = false;
            if (keys[2] && !previous[2]) || mobile_actions & 4 != 0 {
                report = host.invoke("PhysicsSystem.Configure", input.clone())?;
                pos = initial_pos;
                yaw = initial_yaw;
                pitch = initial_pitch;
                clock.reset();
                scheduler_last = std::time::Instant::now();
                changed = true;
            }
            if !paused && report["halted"] != true {
                // Bounded command-line runs compare APIs on identical physical
                // work. Interactive/mobile sessions use elapsed wall time.
                clock.accrue(if fixed_workload { Clock::STEP } else { elapsed });
            } else {
                clock.reset();
            }
            let single_step = paused && ((keys[1] && !previous[1]) || mobile_actions & 2 != 0);
            let physics_start = std::time::Instant::now();
            let mut steps = 0;
            while report["halted"] != true && (clock.due() || (single_step && steps == 0)) {
                let before = report["simulated_seconds"].as_f64().unwrap_or(0.);
                report = host.invoke("PhysicsSystem.Advance", Value::Null)?;
                simulated_advance +=
                    (report["simulated_seconds"].as_f64().unwrap_or(before) - before).max(0.);
                clock.consume();
                steps += 1;
                changed = true;
                // Bounded catch-up: retain the fixed solver step, keep UI live.
                if steps >= 4 || single_step || physics_start.elapsed().as_secs_f64() >= 0.012 {
                    break;
                }
            }
            if steps > 0 {
                physics_seconds = physics_start.elapsed().as_secs_f64();
            }
            if changed {
                let mesh_start = std::time::Instant::now();
                let mut mesh = host.invoke("PhysicsSystem.Mesh", Value::Null)?;
                mesh["entity"] = entity["entity"].clone();
                host.invoke("EntitySystem.Update", mesh)?;
                mesh_seconds = mesh_start.elapsed().as_secs_f64();
            }
            previous = keys;
            let dt = last.elapsed().as_secs_f64().min(0.05);
            last = std::time::Instant::now();
            let flight = &window["flight"];
            yaw += flight["look"][0].as_f64().unwrap_or(0.) * 0.003;
            pitch = (pitch - flight["look"][1].as_f64().unwrap_or(0.) * 0.003).clamp(-1.5, 1.5);
            let axis = |a: usize, b: usize| {
                f64::from(flight["move"][a] == true) - f64::from(flight["move"][b] == true)
            };
            let (f, s, u) = (axis(0, 1), axis(3, 2), axis(4, 5));
            pos[0] += (f * yaw.sin() * pitch.cos() + s * yaw.cos()) * dt * 3.;
            pos[1] += (f * pitch.sin() + u) * dt * 3.;
            pos[2] += (-f * yaw.cos() * pitch.cos() + s * yaw.sin()) * dt * 3.;
            if let Some(remaining) =
                std::time::Duration::from_micros(16_667).checked_sub(frame_start.elapsed())
            {
                std::thread::sleep(remaining);
            }
            performance.record(
                frame_start.elapsed().as_secs_f64(),
                physics_seconds,
                mesh_seconds,
                render_seconds,
                if paused { 0. } else { simulated_advance },
                !paused && report["halted"] != true,
            );
        }
        Ok(
            json!({"mode":"simulation_workshop","physics":report,"rendered_frames":frames,"backend":host.backend(),"performance":performance.report(),"clock_mode":if fixed_workload {"fixed_workload_benchmark"} else {"wall_time_fixed_steps"}}),
        )
    })();
    let mut errors = Vec::new();
    if !entity.is_null() {
        if let Err(e) = host.invoke("AppearanceSystem.Release", entity.clone()) {
            errors.push(e);
        }
        if let Err(e) = host.invoke("EntitySystem.Remove", entity) {
            errors.push(e);
        }
    }
    if let Err(e) = host.invoke("PhysicsSystem.Release", Value::Null) {
        errors.push(e);
    }
    if let Err(e) = host.invoke(
        if ready && execution.is_ok() && errors.is_empty() {
            "AudicleSystem.Finish"
        } else {
            "AudicleSystem.Cancel"
        },
        event,
    ) {
        errors.push(e);
    }
    match execution {
        Ok(mut report) if errors.is_empty() => {
            report["entities_remaining"] = json!(host.entities.len());
            Ok(report)
        }
        result => Err(format!(
            "workshop: {:?}; cleanup: {:?}",
            result.err(),
            errors
        )),
    }
}
