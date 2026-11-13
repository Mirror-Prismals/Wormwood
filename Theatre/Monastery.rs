//! Small, metric Zen compound beside the western residential lane.
use super::*;
#[derive(Clone,Copy,Debug)]
pub(crate) struct Site { pub center:V, pub road:V }
impl Site {
 pub fn incense(&self)->V {add(self.center,[-10.,0.94,-8.])}
 pub fn lighter(&self)->V {add(self.incense(),[0.24,0.,0.])}
}
