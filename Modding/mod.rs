//! Native package discovery and lifecycle. No discovery or calls run on audio callbacks.
use std::{collections::{BTreeMap,BTreeSet},ffi::c_void,path::{Path,PathBuf}};
use serde_json::{json,Value};
use mirror_mod_api::{Api,Buffer,Entry,HostApi,Slice,ABI_VERSION,ENTRY_SYMBOL};
const MAX_MANIFEST:usize=256*1024;
const MAX_REPLY:usize=128*1024*1024;
#[derive(Clone,Debug)]
pub(crate) struct Package {pub id:String,pub name:String,pub version:String,pub root:PathBuf,library:PathBuf,dependencies:Vec<String>,exports:BTreeSet<String>,capabilities:BTreeSet<String>,document_hooks:BTreeMap<String,String>,systems:Vec<Value>}
fn id(value:&str)->bool{!value.is_empty()&&value.len()<=64&&value.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'-'||c==b'_')}
fn names(v:&Value,field:&str)->Result<Vec<String>,String>{v[field].as_array().filter(|a|a.len()<=256).ok_or_else(||format!("mod {field} must be a bounded array"))?.iter().map(|v|v.as_str().filter(|s|!s.is_empty()&&s.len()<=128).map(str::to_owned).ok_or_else(||format!("invalid mod {field}"))).collect()}
impl Package {
 pub(crate) fn read(root:&Path)->Result<Self,String>{
  let root=root.canonicalize().map_err(|e|format!("mod package {}: {e}",root.display()))?;
  let bytes=std::fs::read(root.join("mod.json")).map_err(|e|e.to_string())?;
  if bytes.len()>MAX_MANIFEST{return Err("mod manifest exceeds 256 KiB".into());}
  let v:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
  if v["schema_version"]!=1||v["api_version"]!=ABI_VERSION{return Err("unsupported mod manifest/API version".into());}
  let identifier=v["id"].as_str().filter(|s|id(s)).ok_or("invalid mod id")?.to_owned();
  let name=v["name"].as_str().filter(|s|!s.is_empty()&&s.len()<=128).ok_or("invalid mod name")?.to_owned();
  let version=v["version"].as_str().filter(|s|!s.is_empty()&&s.len()<=64).ok_or("invalid mod version")?.to_owned();
  let key=format!("{}-{}",std::env::consts::OS,std::env::consts::ARCH);
  let relative=Path::new(v["libraries"][&key].as_str().ok_or_else(||format!("mod {identifier} has no library for {key}"))?);
  if relative.is_absolute()||relative.components().any(|c|!matches!(c,std::path::Component::Normal(_))){return Err("mod library must be a relative package path".into());}
  let library=root.join(relative);
  let dependencies=names(&v,"dependencies")?;
  if dependencies.iter().any(|s|!id(s)){return Err("invalid dependency id".into());}
  let exports=names(&v,"exports")?.into_iter().collect();
  let capabilities=names(&v,"capabilities")?.into_iter().collect();
  let document_hooks=serde_json::from_value(v.get("document_hooks").cloned().unwrap_or(json!({}))).map_err(|e|e.to_string())?;
  let systems=serde_json::from_value(v.get("systems").cloned().unwrap_or(json!([]))).map_err(|e|e.to_string())?;
  Ok(Self{id:identifier,name,version,root,library,dependencies,exports,capabilities,document_hooks,systems})
 }
 pub fn asset(&self,relative:&Path)->Result<PathBuf,String>{
  if relative.is_absolute()||relative.components().any(|c|!matches!(c,std::path::Component::Normal(_))){return Err("mod asset must be package-relative".into());}
  let path=self.root.join("Assets").join(relative).canonicalize().map_err(|e|e.to_string())?;
  let assets=self.root.join("Assets").canonicalize().map_err(|e|e.to_string())?;
  if !path.starts_with(assets){return Err("mod asset escapes its package".into());}Ok(path)
 }
}
fn order(packages:&BTreeMap<String,Package>,selected:&[String])->Result<Vec<String>,String>{
 fn visit(id:&str,packages:&BTreeMap<String,Package>,active:&mut BTreeSet<String>,done:&mut BTreeSet<String>,out:&mut Vec<String>)->Result<(),String>{
  if done.contains(id){return Ok(());}
  if !active.insert(id.into()){return Err(format!("mod dependency cycle at {id}"));}
  let p=packages.get(id).ok_or_else(||format!("selected/required mod {id} is not installed"))?;
  for dependency in &p.dependencies{visit(dependency,packages,active,done,out)?;}
  active.remove(id);done.insert(id.into());out.push(id.into());Ok(())
 }
 let(mut active,mut done,mut out)=(BTreeSet::new(),BTreeSet::new(),Vec::new());
 for id in selected{visit(id,packages,&mut active,&mut done,&mut out)?;}Ok(out)
}
struct Native {api:*const Api,state:*mut c_void,library:std::sync::Arc<libloading::Library>}
// All mutable native calls are serialized by the instance mutex. Pure query
// functions do not access instance state and are the worker-safe fast path.
unsafe impl Send for Native {}
thread_local!{static CALL_STACK:std::cell::RefCell<BTreeSet<usize>>=const{std::cell::RefCell::new(BTreeSet::new())};}
struct CallGuard(usize);
impl CallGuard {
 fn enter(key:usize)->Result<Self,String>{CALL_STACK.with(|stack|if stack.borrow_mut().insert(key){Ok(Self(key))}else{Err("recursive calls into the same native mod are not supported".into())})}
}
impl Drop for CallGuard{fn drop(&mut self){CALL_STACK.with(|stack|{stack.borrow_mut().remove(&self.0);});}}
#[derive(Clone)]struct Loaded {package:Package,native:std::sync::Arc<std::sync::Mutex<Native>>}
impl Loaded {
 fn open(package:Package)->Result<Self,String>{unsafe{
  let path=package.library.canonicalize().map_err(|e|format!("mod {} library: {e}",package.id))?;
  if !path.starts_with(&package.root){return Err("mod library escapes package".into());}
  let library=libloading::Library::new(path).map_err(|e|format!("load mod {}: {e}",package.id))?;
  let entry=library.get::<Entry>(ENTRY_SYMBOL).map_err(|e|e.to_string())?;let api=entry();
  if api.is_null()||(*api).abi!=ABI_VERSION{return Err("native mod API mismatch".into());}
  let mut error=Buffer::empty();let root=package.root.to_str().ok_or("mod root must be UTF-8")?;
  let state=((*api).create)(Slice::new(root.as_bytes()),&mut error);
  let message=read_buffer(error,(*api).free_buffer)?;
  if state.is_null(){return Err(format!("mod {}: {}",package.id,String::from_utf8_lossy(&message)));}
  Ok(Self{package,native:std::sync::Arc::new(std::sync::Mutex::new(Native{api,state,library:std::sync::Arc::new(library)}))})
 }}
 fn call(&mut self,host:&HostApi,context:*mut c_void,hook:&str,input:&Value)->Result<Value,String>{unsafe{
  if !self.package.exports.contains(hook){return Err(format!("mod {} does not export {hook}",self.package.id));}
  let _guard=CallGuard::enter(std::sync::Arc::as_ptr(&self.native) as usize)?;
  let native=self.native.lock().map_err(|_|"native mod mutex poisoned")?;let api=native.api;
  let request=serde_json::to_vec(input).map_err(|e|e.to_string())?;let mut output=Buffer::empty();
  let status=((*api).call)(native.state,host,context,Slice::new(hook.as_bytes()),Slice::new(&request),&mut output);
  let bytes=read_buffer(output,(*api).free_buffer)?;
  if status!=0{return Err(format!("mod {}: {}",self.package.id,String::from_utf8_lossy(&bytes)));}
  serde_json::from_slice(&bytes).map_err(|e|format!("mod {} reply: {e}",self.package.id))
 }}
}
impl Drop for Native{fn drop(&mut self){unsafe{((*self.api).destroy)(self.state);}}}
unsafe fn read_buffer(buffer:Buffer,free:unsafe extern "C" fn(Buffer))->Result<Vec<u8>,String>{
 let valid=buffer.len<=buffer.capacity&&buffer.len<=MAX_REPLY&&(buffer.len==0||!buffer.data.is_null());
 if !valid{return Err("invalid/oversized native mod reply".into());}
 let bytes=if buffer.len==0{Vec::new()}else{unsafe{std::slice::from_raw_parts(buffer.data,buffer.len)}.to_vec()};unsafe{free(buffer)};Ok(bytes)
}
unsafe extern "C" fn unavailable(_context:*mut c_void,_hook:Slice,_input:Slice,out:*mut Buffer)->i32{if !out.is_null(){unsafe{*out=Buffer::new(b"Host calls are unavailable during package inspection".to_vec());}}1}
static INSPECTION_API:HostApi=HostApi{abi:ABI_VERSION,invoke:unavailable,free_buffer:mirror_mod_api::free_buffer};
#[derive(Default)]
pub(crate) struct Manager {available:BTreeMap<String,Package>,loaded:Vec<Loaded>}
impl Manager {
 pub fn discover(roots:&[PathBuf],selected:&[String])->Result<Self,String>{
  let available=inventory(roots)?;
  let ordered=order(&available,selected)?;
  let mut loaded=Vec::new();for id in ordered{loaded.push(Loaded::open(available[&id].clone())?);}
  Ok(Self{available,loaded})
 }
 pub fn from_root(root:&Path)->Result<Self,String>{
  let (roots,selected)=selection(root)?;Self::discover(&roots,&selected)
 }
 pub fn enabled(&self,id:&str)->bool{self.loaded.iter().any(|p|p.package.id==id)}
 pub fn provides(&self,capability:&str)->bool{self.loaded.iter().any(|p|p.package.capabilities.contains(capability))}
 pub fn package_root(&self,id:&str)->Option<&Path>{self.loaded.iter().find(|p|p.package.id==id).map(|p|p.package.root.as_path())}
 pub fn inspect(&mut self)->Result<Value,String>{
  let mut descriptions=Vec::new();for p in &mut self.loaded{let hook=format!("{}.Describe",p.package.name);if p.package.exports.contains(&hook){descriptions.push(p.call(&INSPECTION_API,std::ptr::null_mut(),&hook,&Value::Null)?);}}
  Ok(json!({"api_version":ABI_VERSION,"available":self.available.values().map(|p|json!({"id":p.id,"name":p.name,"version":p.version,"root":p.root})).collect::<Vec<_>>(),"enabled":self.loaded.iter().map(|p|&p.package.id).collect::<Vec<_>>(),"descriptions":descriptions}))
 }
}
#[cfg(test)]mod tests{
 use super::*;
 fn package(id:&str,deps:&[&str])->Package{Package{id:id.into(),name:id.into(),version:"1".into(),root:PathBuf::new(),library:PathBuf::new(),dependencies:deps.iter().map(|s|s.to_string()).collect(),exports:Default::default(),capabilities:Default::default(),document_hooks:Default::default(),systems:vec![]}}
 #[test]fn dependency_order_is_deterministic_and_disabled_packages_stay_unloaded(){let p=BTreeMap::from([("a".into(),package("a",&["b"])),("b".into(),package("b",&[]))]);assert!(order(&p,&[]).unwrap().is_empty());assert_eq!(order(&p,&["a".into()]).unwrap(),["b","a"]);assert!(order(&p,&["missing".into()]).is_err());}
 #[test]fn cycles_are_rejected_before_any_library_is_loaded(){let p=BTreeMap::from([("a".into(),package("a",&["b"])),("b".into(),package("b",&["a"]))]);assert!(order(&p,&["a".into()]).unwrap_err().contains("cycle"));}
}

// A process selects its mod set before creating graphics/audio workers. Changes
// require a restart; workers therefore never observe a half-swapped world.
struct Active{capabilities:BTreeSet<String>,queries:Vec<(unsafe extern "C" fn(u32,*const f64,*mut f64)->bool,std::sync::Arc<libloading::Library>)>,services:BTreeMap<String,Loaded>}
static ACTIVE:std::sync::OnceLock<Active>=std::sync::OnceLock::new();
pub(crate) fn provides(capability:&str)->bool{ACTIVE.get().is_some_and(|s|s.capabilities.contains(capability))}
pub(crate) fn query(kind:u32,input:[f64;4])->Option<[f64;4]>{
 let active=ACTIVE.get()?;
 for (query,_) in active.queries.iter().rev(){let mut out=[0.;4];if unsafe{query(kind,input.as_ptr(),out.as_mut_ptr())}&&out.iter().all(|x|x.is_finite()){return Some(out);}}
 None
}
impl Manager {
 pub fn activate(&mut self)->Result<(),String>{
  let capabilities:BTreeSet<_>=self.loaded.iter().flat_map(|p|p.package.capabilities.iter().cloned()).collect();
  if let Some(current)=ACTIVE.get(){if current.services.keys().ne(self.loaded.iter().map(|m|&m.package.id).collect::<BTreeSet<_>>().into_iter()){return Err("changing the loaded mod set requires restarting Mirror".into());}self.loaded=self.loaded.iter().map(|m|current.services[&m.package.id].clone()).collect();return Ok(());}
  for module in &mut self.loaded{if module.package.exports.contains("Mod.Activate"){module.call(&WORLD_API,std::ptr::null_mut(),"Mod.Activate",&Value::Null)?;}}
  let queries=self.loaded.iter().filter_map(|p|{let n=p.native.lock().unwrap();unsafe{(*n.api).query}.map(|q|(q,n.library.clone()))}).collect();
  let services=self.loaded.iter().map(|p|(p.package.id.clone(),p.clone())).collect();
  let _=ACTIVE.set(Active{capabilities,queries,services});Ok(())
 }
 pub fn content_root(&self,id:&str)->Option<PathBuf>{self.package_root(id).map(|p|p.join("Assets"))}
}
unsafe extern "C" fn host_service(_context:*mut c_void,hook:Slice,input:Slice,out:*mut Buffer)->i32{
 let result=std::panic::catch_unwind(||->Result<Value,String>{
  let hook=std::str::from_utf8(unsafe{hook.bytes()}).map_err(|e|e.to_string())?;
  let input:Value=serde_json::from_slice(unsafe{input.bytes()}).map_err(|e|e.to_string())?;
  match hook{"Mirror.TerrainHeight"=>{let point=crate::audio::vector(&input)?;Ok(json!(crate::terrain::height(point)))},
  "Mirror.Invoke"=>{
   let host=unsafe{(_context as *mut crate::host::Host).as_mut()}.ok_or("Host calls require an active System callback")?;
   let parent=host.active_audicle;host.active_audicle=input["audicle"].as_u64();
   let result=host.invoke(input["hook"].as_str().ok_or("SDK hook name")?,input["input"].clone());host.active_audicle=parent;result
  },
  "Mirror.Trace"=>{
   let host=unsafe{(_context as *mut crate::host::Host).as_mut()}.ok_or("trace requires active Host")?;
   let p=crate::audio::vector(&input["position"])?;let d=crate::audio::vector(&input["direction"])?;let distance=input["distance"].as_f64().ok_or("trace distance")?;
   Ok(host.theatre.venue.as_ref().and_then(|v|v.trace_local(p,d,distance)).map_or(Value::Null,|h|json!({"position":h.position,"normal":h.normal,"distance":h.distance})))
  },_=>Err(format!("unsupported Mirror SDK service {hook}"))}
 });
 let(status,bytes)=match result{Ok(Ok(v))=>(0,serde_json::to_vec(&v).unwrap()),Ok(Err(e))=>(1,e.into_bytes()),Err(_)=>(1,b"Mirror SDK service panicked".to_vec())};
 if !out.is_null(){unsafe{*out=Buffer::new(bytes);}}status
}
static WORLD_API:HostApi=HostApi{abi:ABI_VERSION,invoke:host_service,free_buffer:mirror_mod_api::free_buffer};
impl Manager{
 pub fn call(&mut self,id:&str,hook:&str,input:&Value)->Result<Value,String>{
  self.loaded.iter_mut().find(|p|p.package.id==id).ok_or_else(||format!("mod {id} is not enabled"))?.call(&WORLD_API,std::ptr::null_mut(),hook,input)
 }
}
pub(crate) fn call_selected(id:&str,hook:&str,input:&Value)->Result<Value,String>{
 if let Some(mut service)=ACTIVE.get().and_then(|a|a.services.get(id)).cloned(){return service.call(&WORLD_API,std::ptr::null_mut(),hook,input);}
 let root=std::env::var_os("WORMWOOD_ASSET_ROOT").map(PathBuf::from).unwrap_or_else(||PathBuf::from(env!("CARGO_MANIFEST_DIR")));
 let mut manager=Manager::from_root(&root)?;manager.call(id,hook,input)
}

pub(crate) fn patch_document(root:&Path,target:&str,value:Value)->Result<Value,String>{
 let mut manager=Manager::from_root(root)?;let mut value=value;
 for module in &mut manager.loaded{if let Some(hook)=module.package.document_hooks.get(target).cloned(){value=module.call(&WORLD_API,std::ptr::null_mut(),&hook,&value)?;}}Ok(value)
}

impl Manager{
 pub(crate) fn call_host(&mut self,id:&str,hook:&str,input:&Value,context:*mut c_void)->Result<Value,String>{
  self.loaded.iter_mut().find(|p|p.package.id==id).ok_or_else(||format!("mod {id} is not enabled"))?.call(&WORLD_API,context,hook,input)
 }
}

/// Load declarative System extensions only from explicitly selected packages.
/// Libraries cannot replace a built-in System or bypass the Host call graph.
impl Manager {
 pub(crate) fn system_extensions(&self)->Result<Vec<(String,Value,Value)>,String>{
  let mut result=Vec::new();for module in &self.loaded{result.extend(module.package.system_extensions()?);}Ok(result)
 }
}

fn inventory(roots:&[PathBuf])->Result<BTreeMap<String,Package>,String>{
  let mut available=BTreeMap::new();
  for root in roots{
   if !root.exists(){continue;}
   let dirs=if root.join("mod.json").is_file(){vec![root.clone()]}else{std::fs::read_dir(root).map_err(|e|e.to_string())?.filter_map(Result::ok).map(|e|e.path()).filter(|p|p.join("mod.json").is_file()).collect()};
   for root in dirs{let p=Package::read(&root)?;if available.insert(p.id.clone(),p).is_some(){return Err("duplicate installed mod id".into());}}
  }
 Ok(available)
}
fn selection(root:&Path)->Result<(Vec<PathBuf>,Vec<String>),String>{
  let mut roots=vec![root.join("Mods")];if let Some(paths)=std::env::var_os("MIRROR_MOD_PATH"){roots.extend(std::env::split_paths(&paths));}
  let selected=if let Ok(value)=std::env::var("MIRROR_MODS"){value.split(',').filter(|s|!s.is_empty()).map(str::to_owned).collect()}
   else{let p=root.join("Mods/enabled.json");if p.is_file(){let v:Value=serde_json::from_slice(&std::fs::read(p).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;if v["schema_version"]!=1{return Err("unsupported mod selection schema".into());}names(&v,"enabled")?}else{Vec::new()}};
  if selected.iter().any(|s|!id(s)){return Err("invalid enabled mod id".into());}Ok((roots,selected))
}
/// Metadata only: do not execute any native code during intent inspection.
pub(crate) fn selected_metadata(root:&Path)->Result<Vec<Package>,String>{
 let (roots,selected)=selection(root)?;let available=inventory(&roots)?;
 Ok(order(&available,&selected)?.iter().map(|id|available[id].clone()).collect())
}

impl Package{pub(crate) fn system_extensions(&self)->Result<Vec<(String,Value,Value)>,String>{
  let mut result=Vec::new();
  for entry in &self.systems {
   let relative=Path::new(entry["contract"].as_str().ok_or("mod System contract path")?);
   if relative.is_absolute()||relative.components().any(|c|!matches!(c,std::path::Component::Normal(_))){return Err("mod contract must be package relative".into());}
   let path=self.root.join(relative).canonicalize().map_err(|e|e.to_string())?;
   if !path.starts_with(&self.root){return Err("mod contract escapes package".into());}
   let bytes=std::fs::read(path).map_err(|e|e.to_string())?;
   if bytes.len()>MAX_MANIFEST{return Err("mod contract exceeds limit".into());}
   let contract:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
   let system=contract["system"].as_str().ok_or("mod System name")?;
   let signatures=entry["functions"].as_object().ok_or("mod function signatures")?;
   for name in signatures.keys(){if !self.exports.contains(&format!("{system}.{name}")){return Err(format!("mod {} does not export {system}.{name}",self.id));}}
   let metadata=json!({"layer":contract["layer"],"functions":signatures});
   result.push((self.id.clone(),contract,metadata));
  }Ok(result)
}}
