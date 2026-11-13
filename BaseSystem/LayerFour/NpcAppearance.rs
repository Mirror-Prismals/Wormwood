//! Compatibility facade for the optional native mod's actor appearance.
use serde_json::Value;
pub(crate) fn spec(color:[f32;3])->Value{crate::modding::call_selected("prismals","Prismals.NpcSpec",&serde_json::json!(color)).expect("Prismals actor appearance requires its enabled package")}
