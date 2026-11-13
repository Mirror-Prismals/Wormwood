//! Visible draw rate; wall time includes waits and idle periods.
#[derive(Default)]
pub(crate) struct DrawRate { start:f64, frames:u64 }
impl DrawRate {
    pub(crate) fn observe(&mut self, now:f64, drawn:bool)->Option<f64> {
        if !now.is_finite() || now<self.start {return None;}
        self.frames+=u64::from(drawn);
        let elapsed=now-self.start;
        if elapsed<0.5 {return None;}
        let rate=self.frames as f64/elapsed;self.start=now;self.frames=0;Some(rate)
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn only_actual_draws_count_and_idle_reports_zero() {
        let mut r=DrawRate::default();
        for i in 1..50 {assert_eq!(r.observe(i as f64/100.,i%2==0),None);}
        assert_eq!(r.observe(0.5,true),Some(50.));
        assert_eq!(r.observe(1.,false),Some(0.));
    }
    #[test] fn stalls_stay_in_the_rate_denominator() {
        let mut r=DrawRate::default();
        for i in 1..10 {r.observe(i as f64/100.,true);}
        assert_eq!(r.observe(2.,true),Some(5.));
    }
}
