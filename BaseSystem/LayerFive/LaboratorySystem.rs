//! Compatibility entry points; Prismals owns encounter state and behavior.
use crate::host::Host;
use serde_json::Value;
#[derive(Default)]pub(crate) struct State{last:Value}
impl State{pub(crate) fn dispatch(&mut self,host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{let value=host.mod_call("prismals",&format!("LaboratorySystem.{hook}"),input)?;self.last=value.clone();Ok(value)}}
// General half-turn transform retained for the navigation adapter.
pub(crate) fn map_direction(v:[f64;3])->[f64;3]{[-v[0],v[1],-v[2]]}
pub(crate) fn map_point(p:[f64;3],source:[f64;3],destination:[f64;3])->[f64;3]{crate::audio::add(destination,map_direction(crate::audio::sub(p,source)))}
