//! Surveyed town laboratory, temporary zoo and paired yarn installation.
use super::*;
#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub lab: V,
    pub zoo: V,
    pub lab_portal: V,
    pub zoo_portal: V,
    pub lab_vertices: Vec<[f32;7]>,
    pub zoo_vertices: Vec<[f32;7]>,
}
impl Site {
    pub fn contains_lab(&self,p:V)->bool {
        (p[0]-self.lab[0]).abs()<14.8 && p[2]>self.lab[2]-10.5 && p[2]<self.lab[2]+11.5 && p[1]>self.lab[1] && p[1]<self.lab[1]+7.5
    }
    pub fn researcher(&self)->V {add(self.lab,[-3.,0.8,3.])}
    pub fn mouse_start(&self)->V {add(self.lab,[-1.,0.,3.])}
    pub fn zoo_home(&self)->V {add(self.zoo,[-6.,0.,4.])}
}
