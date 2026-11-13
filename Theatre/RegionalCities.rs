//! Generic native-mod scene cache; authored city construction belongs to Prismals.
use super::*;
use std::sync::{Arc,OnceLock};
pub(crate) const GLASS_TAG:f32=70.;pub(crate) const CITY_TAG:f32=71.;
pub(crate) struct City{pub center:V,pub origin:V,pub venue:Arc<Venue>}
pub(crate) fn cities()->&'static [City]{
 static CITIES:OnceLock<Vec<City>>=OnceLock::new();
 CITIES.get_or_init(||{
  let rows=crate::modding::call_selected("prismals","Prismals.BuildCities",&Value::Null).expect("load selected Prismals cities");
  rows.as_array().expect("city array").iter().map(|row|{
   let mut venue=Venue::empty_mod_scene();venue.apply_mod_scene(&row["scene"]).expect("valid native city");
   City{center:vector(&row["center"]).unwrap(),origin:vector(&row["origin"]).unwrap(),venue:Arc::new(venue)}
  }).collect()
 })
}
pub(crate) fn nearby(eye:V,range:f64)->Option<&'static City>{
 if !crate::modding::provides("regional_cities")||crate::terrain::nightworld::active(eye){return None;}
 cities().iter().find(|c|length(sub(eye,c.center))<range)
}
