//! Compatibility entry points; Prismals owns incense behavior and its clock.
use crate::host::Host;
use serde_json::Value;
#[derive(Default)]pub(crate) struct State{last:Value,pub lit:Option<f64>,pub ignition:Option<f64>,pub spent:bool,pub lighter:bool}
impl State{
 pub(crate) fn audicle(&self)->Option<u64>{self.last["audicle"].as_u64()}
 pub(crate) fn dispatch(&mut self,host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{let value=host.mod_call("prismals",&format!("MonasterySystem.{hook}"),input)?;self.last=value.clone();Ok(value)}
}
