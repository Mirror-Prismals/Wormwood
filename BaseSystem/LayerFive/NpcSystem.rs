//! Compatibility entry points; Prismals owns visitor definitions and simulation.
use crate::host::Host;
use serde_json::Value;
#[derive(Default)]pub(crate) struct State{last:Value}
impl State{pub(crate) fn dispatch(&mut self,host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{let value=host.mod_call("prismals",&format!("NpcSystem.{hook}"),input)?;self.last=value.clone();Ok(value)}}
