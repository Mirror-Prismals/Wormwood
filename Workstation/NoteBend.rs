//! Smooth clip-local note bends, in semitones; standard MIDI range is +/-2.
use serde_json::Value;
pub(crate) fn read(v: &Value) -> Result<f64, String> {
    if v.is_null() { return Ok(0.); }
    v.as_f64().filter(|x| x.is_finite() && (-2. ..=2.).contains(x))
        .ok_or_else(|| "bend_semitones must be finite within -2..2".into())
}
pub(crate) fn write(mut note: Value, bend: f64) -> Value {
    if bend.abs()>1e-9 { note["bend_semitones"]=serde_json::json!(bend); }
    else { note.as_object_mut().unwrap().remove("bend_semitones"); }
    note
}
pub(crate) fn value(end: f64, fraction: f64) -> f64 {
    let t = fraction.clamp(0., 1.);
    end * t * t * (3. - 2. * t)
}

// Integrated exponential of the smoothstep, with a degree-18 polynomial.
// |ln(2)*bend/12| <= .116; six Taylor terms bound frequency error below 6e-11.
// Precomputed on preparation, evaluated without exp/allocation per sample.
pub(crate) fn phase_coefficients(end: f64) -> [f64;20] {
    let k=end*std::f64::consts::LN_2/12.;
    let mut sum=[0.;19]; let mut term=[0.;19];term[0]=1.;sum[0]=1.;
    for order in 1..=6 {let mut next=[0.;19];
        for i in 0..=18 {if i+2<=18 {next[i+2]+=term[i]*3.*k/order as f64;}
            if i+3<=18 {next[i+3]-=term[i]*2.*k/order as f64;}}
        term=next;for i in 0..19 {sum[i]+=term[i];}
    }
    let mut integral=[0.;20];for i in 0..19 {integral[i+1]=sum[i]/(i+1)as f64;}integral
}
pub(crate) fn phase(coefficients: &[f64;20], fraction: f64) -> f64 {
    let t=fraction.clamp(0.,1.);
    let integrated=coefficients.iter().rev().fold(0.,|sum,c|sum*t+c);
    let end_frequency=coefficients.iter().enumerate().skip(1).map(|(i,c)|c*i as f64).sum::<f64>();
    integrated+(fraction-1.).max(0.)*end_frequency
}

#[cfg(test)] mod tests {
 use super::*;use serde_json::json;
 #[test] fn bend_validation_and_phase_are_seek_deterministic(){
  assert_eq!(read(&Value::Null).unwrap(),0.);for x in [json!(3.),json!(-3.),json!("1")]{assert!(read(&x).is_err());}
  for bend in [-2.,0.,2.] {let c=phase_coefficients(bend);
   for t in [0.01,0.25,0.5,0.99,1.1] {let h=1e-6;let derivative=(phase(&c,t+h)-phase(&c,t-h))/(2.*h);
    let expected=2f64.powf(value(bend,t)/12.);assert!((derivative-expected).abs()<1e-8);}
  }
 }
}
