//! Intended behavior in System contracts. Inspection never starts Systems or hosts.
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

type Graph = BTreeMap<String, BTreeSet<String>>;
fn read(root: &Path, path: &str) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?)
        .map_err(|e| format!("{path}: {e}"))
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} must be nonempty text"))
}
fn strings(v: &Value) -> Result<Vec<String>, String> {
    let values = v.as_array().ok_or("expected an array of names")?;
    let mut result = Vec::new();
    for v in values {
        let s = v
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("expected nonempty names")?;
        if result.iter().any(|n| n == s) {
            return Err(format!("duplicate name {s}"));
        }
        result.push(s.to_owned());
    }
    Ok(result)
}
fn fields(v: &Value, allowed: &[&str]) -> Result<(), String> {
    for key in v.as_object().ok_or("expected an object")?.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown intent field {key}"));
        }
    }
    Ok(())
}
fn identifier(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
fn safe_path(s: &str) -> bool {
    !s.is_empty()
        && !s.contains(['\\', ':'])
        && Path::new(s)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}
fn hook_exists(catalog: &Value, qualified: &str) -> bool {
    qualified
        .split_once('.')
        .is_some_and(|(s, f)| catalog["systems"][s]["functions"].get(f).is_some())
}
/// Missing descriptions are reported by inspection, never interpreted as retirement.
pub(super) fn validate_contract(
    system: &str,
    contract: &Value,
    catalog: &Value,
) -> Result<(), String> {
    let Some(intent) = contract.get("intent") else {
        return Ok(());
    };
    let validate = || -> Result<(), String> {
        fields(intent, &["schema_version", "behaviors"])?;
        if intent["schema_version"] != 1 {
            return Err("unsupported intent schema".into());
        }
        let behaviors = intent["behaviors"]
            .as_array()
            .ok_or("intent behaviors must be an array")?;
        let mut ids = BTreeSet::new();
        for b in behaviors {
            fields(
                b,
                &[
                    "id",
                    "description",
                    "retention",
                    "selection",
                    "implements",
                    "sources",
                    "checks",
                ],
            )?;
            let id = text(b, "id")?;
            // Behavior IDs are stable labels, not Rust/System hook identifiers.
            if !identifier(&id.replace('-', "_")) || !ids.insert(id) {
                return Err(format!("invalid or duplicate behavior {id}"));
            }
            text(b, "description")?;
            text(b, "selection")?;
            let retention = text(b, "retention")?;
            if !matches!(retention, "supported" | "experimental" | "retired") {
                return Err(format!("invalid retention {retention}"));
            }
            let hooks = strings(&b["implements"])?;
            if hooks.is_empty() {
                return Err(format!("{id} must identify its named functions"));
            }
            for f in hooks {
                if !identifier(&f)
                    || (retention != "retired" && !hook_exists(catalog, &format!("{system}.{f}")))
                {
                    return Err(format!("{id} identifies unknown function {f}"));
                }
            }
            let sources = strings(&b["sources"])?;
            if sources.is_empty() || sources.iter().any(|s| !safe_path(s)) {
                return Err(format!("{id} needs relative source file paths"));
            }
            let checks = b["checks"]
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or("behavior needs verification obligations")?;
            let mut check_ids = BTreeSet::new();
            for check in checks {
                let check_id = text(check, "id")?;
                if !identifier(check_id) || !check_ids.insert(check_id) {
                    return Err("invalid or duplicate check id".into());
                }
                text(check, "expectation")?;
                match text(check, "kind")? {
                    "declared_calls" => {
                        fields(check, &["id", "kind", "expectation", "targets"])?;
                        let targets = strings(&check["targets"])?;
                        if targets.is_empty() {
                            return Err("declared_calls needs targets".into());
                        }
                        for target in targets {
                            if !hook_exists(catalog, &target) && retention != "retired" {
                                return Err(format!("unknown required hook {target}"));
                            }
                        }
                    }
                    "behavior" => {
                        fields(check, &["id", "kind", "expectation", "procedure"])?;
                        text(check, "procedure")?;
                    }
                    kind => return Err(format!("unknown verification kind {kind}")),
                }
            }
        }
        Ok(())
    };
    validate().map_err(|e| format!("{system} intent: {e}"))
}
fn reachable(graph: &Graph, roots: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut seen = BTreeSet::new();
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        if let Some(calls) = graph.get(&name) {
            pending.extend(calls.iter().cloned());
        }
    }
    seen
}
fn source_inventory(
    root: &Path,
    folder: &Path,
    result: &mut BTreeSet<String>,
) -> Result<(), String> {
    if !folder.is_dir() {
        return Ok(());
    }
    for item in fs::read_dir(folder).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let kind = item.file_type().map_err(|e| e.to_string())?;
        let path = item.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            source_inventory(root, &path, result)?;
        } else if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|s| matches!(s, "rs" | "m" | "mm" | "c" | "cc" | "cpp" | "h" | "hpp" | "vert" | "frag" | "wgsl" | "ck"))
        {
            result.insert(
                path.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    Ok(())
}
/// Compares declarations and source bindings. Behavioral equivalence is not inferred.
pub(crate) fn inspect(root: &Path) -> Result<Value, String> {
    let mut catalog = read(root, "Procedures/functionregistry.json")?;
    if catalog["schema_version"] != 1 {
        return Err("unsupported function registry version".into());
    }
    let mut registry = read(root, "BaseSystem/registry.json")?;
    let packages=crate::modding::selected_metadata(root)?;
    let mut native_contracts=BTreeMap::new();
    for package in &packages {for (_,contract,metadata) in package.system_extensions()? {
        let name=contract["system"].as_str().ok_or("native System name")?.to_owned();
        if catalog["systems"].get(&name).is_some(){return Err(format!("mod cannot replace registered System {name}"));}
        registry.as_object_mut().ok_or("System registry")?.insert(name.clone(),json!(true));
        catalog["systems"].as_object_mut().ok_or("function registry")?.insert(name.clone(),metadata);
        native_contracts.insert(name,contract);
    }}
    let systems = catalog["systems"]
        .as_object()
        .ok_or("missing function registry Systems")?;
    let registry = registry.as_object().ok_or("invalid System registry")?;
    if systems.keys().collect::<BTreeSet<_>>() != registry.keys().collect::<BTreeSet<_>>()
        || registry.values().any(|v| !v.is_boolean())
    {
        return Err("function registry and boolean System registry must agree".into());
    }
    let mut graph = Graph::new();
    let mut contracts = BTreeMap::new();
    let mut external_roots = BTreeSet::new();
    for (system, entry) in systems {
        if !identifier(system) {
            return Err("invalid System name".into());
        }
        let contract = if let Some(c)=native_contracts.get(system){c.clone()}else{read(root, &format!("Systems/{system}.json"))?};
        if contract["system"] != system.as_str() || contract["layer"] != entry["layer"] {
            return Err(format!("{system} contract mismatch"));
        }
        let functions = entry["functions"]
            .as_object()
            .ok_or("missing registered functions")?;
        let declarations = contract["functions"]
            .as_object()
            .ok_or("missing contracted functions")?;
        if functions.keys().collect::<BTreeSet<_>>() != declarations.keys().collect::<BTreeSet<_>>()
        {
            return Err(format!("{system} function catalog mismatch"));
        }
        validate_contract(system, &contract, &catalog)?;
        for (function, declaration) in declarations {
            graph.insert(
                format!("{system}.{function}"),
                strings(&declaration["calls"])?.into_iter().collect(),
            );
        }
        for field in ["init_steps", "entry_steps"] {
            if let Some(steps) = contract.get(field) {
                for f in strings(steps)? {
                    external_roots.insert(format!("{system}.{f}"));
                }
            }
        }
        contracts.insert(system.clone(), contract);
    }
    for (hook, calls) in &graph {
        for target in calls {
            if !graph.contains_key(target) {
                return Err(format!("{hook} declares unknown target {target}"));
            }
        }
    }
    for hook in &external_roots {
        if !graph.contains_key(hook) {
            return Err(format!("unknown entry/startup function {hook}"));
        }
    }
    let mut inventory = BTreeSet::new();
    for dir in [
        "BaseSystem",
        "Render",
        "Host",
        "Platform",
        "Structures",
        "Terrain",
        "Physics",
        "Water",
        "Vegetation",
        "Audio",
        "Procedures/Shaders",
        "Procedures/Water/Shaders",
        "Procedures/chuck",
    ] {
        source_inventory(root, &root.join(dir), &mut inventory)?;
    }
    for name in ["BaseEntity.rs", "lib.rs", "main.rs", "build.rs"] {
        if root.join(name).is_file() {
            inventory.insert(name.into());
        }
    }
    let mut mapped = BTreeSet::new();
    let mut sources = BTreeSet::new();
    let mut retained = BTreeSet::new();
    let mut retired = BTreeSet::new();
    let mut reports = Vec::new();
    let mut failures = 0;
    let mut obligations = 0;
    for (system, contract) in &contracts {
        let Some(behaviors) = contract["intent"]["behaviors"].as_array() else {
            continue;
        };
        for b in behaviors {
            let hooks: Vec<String> = strings(&b["implements"])?
                .iter()
                .map(|f| format!("{system}.{f}"))
                .collect();
            let active = b["retention"] != "retired";
            let destinations = reachable(&graph, hooks.clone());
            mapped.extend(hooks.iter().cloned());
            if active {
                retained.extend(hooks.iter().cloned());
            } else {
                retired.extend(hooks.iter().cloned());
            }
            let mut checks = Vec::new();
            for check in b["checks"].as_array().unwrap() {
                let mut result = check.clone();
                if check["kind"] == "declared_calls" {
                    let missing: Vec<_> = strings(&check["targets"])?
                        .into_iter()
                        .filter(|t| !destinations.contains(t))
                        .collect();
                    result["status"] = json!(if missing.is_empty() {
                        "declarations_match"
                    } else {
                        "declaration_mismatch"
                    });
                    result["missing_targets"] = json!(missing);
                    if active && result["status"] == "declaration_mismatch" {
                        failures += 1;
                    }
                } else {
                    result["status"] = json!("not_checked");
                    if active {
                        obligations += 1;
                    }
                }
                checks.push(result);
            }
            let mut bindings = Vec::new();
            for source in strings(&b["sources"])? {
                sources.insert(source.clone());
                let path=if let Some((id,relative))=source.strip_prefix("ModSources/").and_then(|s|s.split_once('/')) {
                    let installed=packages.iter().find(|p|p.id==id).map(|p|if relative.starts_with("Assets/"){p.root.join(relative)}else{p.root.join("Source").join(relative)});
                    installed.filter(|p|p.is_file()).unwrap_or_else(||if id=="prismals"{root.parent().unwrap_or(root).join("Prismals").join(relative)}else{root.join(&source)})
                }else{root.join(&source)};
                let exists = path.is_file();
                let optional_package=source.strip_prefix("ModSources/").and_then(|s|s.split_once('/')).is_some_and(|(id,_)|!packages.iter().any(|p|p.id==id));
                if active && !exists && !optional_package {
                    failures += 1;
                }
                bindings
                    .push(json!({"path":source,"status":if exists {"present"} else if optional_package {"external_package_not_installed"} else {"missing"}}));
            }
            reports.push(json!({"id":format!("{system}.{}",b["id"].as_str().unwrap()),"description":b["description"],"retention":b["retention"],"selection":b["selection"],"configured_enabled":registry[system],"implements":hooks,"sources":bindings,"checks":checks,"behavior_status":"not_checked"}));
        }
    }
    let unmapped: Vec<_> = graph
        .keys()
        .filter(|h| !mapped.contains(*h))
        .cloned()
        .collect();
    // Undocumented functions and every declared entry are conservatively retained.
    // A disabled System is included; its current toggle is never a retirement signal.
    retained.extend(unmapped.iter().cloned());
    retained.extend(external_roots.iter().cloned());
    let protected = reachable(&graph, retained);
    let candidates: Vec<_> = retired
        .iter()
        .filter(|h| graph.contains_key(*h) && !protected.contains(*h))
        .cloned()
        .collect();
    let blocked: Vec<_> = retired
        .iter()
        .filter(|h| protected.contains(*h))
        .cloned()
        .collect();
    let unbound: Vec<_> = inventory.difference(&sources).cloned().collect();
    Ok(
        json!({"schema_version":1,"inspection":"application_intent","application":read(root,"Project.json")?["display_name"],"behavior_status":"not_checked","scope":"Declared call routes and source-file bindings; every System is included regardless of runtime state. No Systems, graphics, audio, tests, or AI reviewers are started.","summary":{"documented_behaviors":reports.len(),"registered_functions":graph.len(),"unmapped_functions":unmapped.len(),"unmapped_source_files":unbound.len(),"active_structural_mismatches":failures,"behavior_checks_pending":obligations},"behaviors":reports,"unmapped_functions":unmapped,"unmapped_source_files":unbound,"pruning_review":{"basis":"Declared routes only; review source callers, callbacks, helpers, platforms and assets before removal. No deletion is performed.","retired_without_declared_users":candidates,"retired_still_referenced":blocked},"limits":["A permitted call is not evidence that the source makes that call.","A source-file binding does not establish that every part of the file serves the behavior.","Unmapped and disabled code is not automatically obsolete.","Behavior descriptions require source review and the listed verification procedures; this inspection does not prove implementation equivalence."]}),
    )
}
pub(crate) fn plain(report: &Value) -> String {
    let mut out = format!(
        "Application intent: {}\n{}\n\n",
        report["application"].as_str().unwrap_or("application"),
        report["scope"].as_str().unwrap()
    );
    let s = &report["summary"];
    out += &format!(
        "{} described behaviors; {} registered functions.\n{} functions and {} source files need intent links.\n{} structural mismatches; {} behavior checks pending.\n\n",
        s["documented_behaviors"],
        s["registered_functions"],
        s["unmapped_functions"],
        s["unmapped_source_files"],
        s["active_structural_mismatches"],
        s["behavior_checks_pending"]
    );
    for b in report["behaviors"].as_array().unwrap() {
        out += &format!(
            "{} [{}; System currently {}]\n{}\nSelection: {}\n",
            b["id"].as_str().unwrap(),
            b["retention"].as_str().unwrap(),
            if b["configured_enabled"] == true {
                "enabled"
            } else {
                "disabled"
            },
            b["description"].as_str().unwrap(),
            b["selection"].as_str().unwrap()
        );
        out += &format!(
            "Functions: {}\n",
            strings(&b["implements"]).unwrap().join(", ")
        );
        for source in b["sources"].as_array().unwrap() {
            out += &format!(
                "Source {}: {}\n",
                source["status"].as_str().unwrap(),
                source["path"].as_str().unwrap()
            );
        }
        for c in b["checks"].as_array().unwrap() {
            out += &format!(
                "  {}: {}\n    {}\n",
                c["id"].as_str().unwrap(),
                c["status"].as_str().unwrap(),
                c["expectation"].as_str().unwrap()
            );
            if let Some(p) = c["procedure"].as_str() {
                out += &format!("    Required check: {p}\n");
            }
            if c["status"] == "declaration_mismatch" {
                out += &format!("    Missing declared routes: {}\n", c["missing_targets"]);
            }
        }
        out += "\n";
    }
    out += &format!(
        "Functions needing intent links: {}\nSource files needing intent links: {}\nRetired functions without declared users: {}\nRetired functions still referenced: {}\n\n",
        report["unmapped_functions"],
        report["unmapped_source_files"],
        report["pruning_review"]["retired_without_declared_users"],
        report["pruning_review"]["retired_still_referenced"]
    );
    for limit in report["limits"].as_array().unwrap() {
        out += &format!("{}\n", limit.as_str().unwrap());
    }
    out
}
