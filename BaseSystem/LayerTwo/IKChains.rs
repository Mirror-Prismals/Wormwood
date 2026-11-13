//! Stateless bounded limb batches share the player FABRIK implementation.
//! Seeds and temporal ownership belong to the calling animal, not player IK state.
use super::{fabrik, safe};
use crate::base_system::layer_two::player_model_system::math::*;
use serde_json::{Value, json};
pub(crate) fn solve(input: &Value) -> Result<Value, String> {
    let rows = input["chains"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 8)
        .ok_or("one to eight IK chains required")?;
    let mut solved = Vec::with_capacity(rows.len());
    for row in rows {
        let point = |key: &str| -> Result<V, String> {
            let p = vector(&row[key])?;
            if p.iter().any(|v| v.abs() > 16.) {
                return Err("IK chain coordinates must be local and bounded".into());
            }
            Ok(p)
        };
        let root = point("root")?;
        let mut mid = point("mid")?;
        let mut end = point("end")?;
        let requested = point("target")?;
        let pole = point("pole")?;
        let lengths: [f64; 2] = serde_json::from_value(row["lengths"].clone())
            .map_err(|_| "two IK bone lengths required")?;
        if lengths
            .iter()
            .any(|v| !v.is_finite() || !(0.02..=4.).contains(v))
        {
            return Err("bounded positive IK bone lengths required".into());
        }
        let [a, b] = lengths;
        let delta = sub(requested, root);
        let distance = len(delta);
        let low = (a - b).abs() + 0.00001;
        let high = a + b - 0.00001;
        let direction = safe(delta, safe(sub(end, root), [0., -1., 0.]));
        let target = add(root, mul(direction, distance.clamp(low, high)));
        fabrik(
            root,
            target,
            pole,
            a,
            b,
            48,
            &mut mid,
            &mut end,
            [0., 1., 0.],
            [0., 0., 1.],
        );
        solved.push(json!({"joints":[root,mid,end],"reachable":(low..=high).contains(&distance),"error":len(sub(end,requested))}));
    }
    Ok(json!({"chains":solved}))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ik_chain_batch_preserves_lengths_and_reports_unreachable_targets() {
        for target in [[0.3, 0.1, 0.2], [0., -3., 0.], [0., 1.2, 0.]] {
            let input = json!({"chains":[{"root":[0.,1.2,0.],"mid":[0.,0.6,0.2],"end":[0.,0.,0.],"target":target,"pole":[0.,0.6,1.],"lengths":[0.63,0.63]}]});
            let result = solve(&input).unwrap();
            let chain = &result["chains"][0];
            let r = vector(&chain["joints"][0]).unwrap();
            let m = vector(&chain["joints"][1]).unwrap();
            let e = vector(&chain["joints"][2]).unwrap();
            assert!((len(sub(m, r)) - 0.63).abs() < 1e-5);
            assert!((len(sub(e, m)) - 0.63).abs() < 1e-4);
            if target[1] < -2. {
                assert_eq!(chain["reachable"], false);
            } else if target[1] < 1. {
                assert!(chain["error"].as_f64().unwrap() < 0.002);
            }
        }
        assert!(solve(&json!({"chains":[]})).is_err());
        assert!(solve(&json!({"chains":[{"root":[0.,0.,0.],"mid":[0.,1.,0.],"end":[0.,2.,0.],"target":[1.,0.,0.],"pole":[0.,0.,1.],"lengths":[0.,1.]}]})).is_err());
    }
}
