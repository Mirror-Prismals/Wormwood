//! Bounded UIOnly payload for Cardinal catalog labels, insert names and stock graphs.
use serde_json::Value;
fn numbers(v: &Value, n: usize) -> bool {
    v.as_array().is_some_and(|a| {
        a.len() == n
            && a.iter().all(|x| {
                x.as_f64()
                    .is_some_and(|f| f.is_finite() && f.abs() <= 32768.)
            })
    })
}
fn rect(v: &Value) -> bool {
    numbers(v, 4) && v[2].as_f64().unwrap() >= 0. && v[3].as_f64().unwrap() >= 0.
}
fn text(v: &Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() <= 1024 && !s.chars().any(char::is_control))
}
pub(crate) fn valid(v: &Value) -> bool {
    if v.is_null() {
        return true;
    }
    v["groups"].as_array().is_some_and(|groups| {
        groups.len() <= 16
            && groups.iter().all(|g| {
                rect(&g["clip"])
                    && g["labels"].as_array().is_some_and(|labels| {
                        labels.len() <= 256
                            && labels.iter().all(|l| {
                                text(&l["text"])
                                    && (l["icon"].is_null() || rect(&l["icon"]))
                                    && rect(&l["rect"])
                                    && l["size"]
                                        .as_f64()
                                        .is_some_and(|n| n.is_finite() && n > 0. && n <= 128.)
                                    && numbers(&l["color"], 3)
                            })
                    })
                    && (g["browser_faces"].is_null()
                        || g["browser_faces"].as_array().is_some_and(|faces| {
                            faces.len() <= 8192
                                && faces.iter().all(|f| {
                                    rect(&f["rect"])
                                        && numbers(&f["color"], 3)
                                        && f["color"]
                                            .as_array()
                                            .unwrap()
                                            .iter()
                                            .all(|v| (0.0..=255.0).contains(&v.as_f64().unwrap()))
                                        && f["radius"].as_f64().is_some_and(|n| {
                                            n.is_finite() && (0.0..=128.0).contains(&n)
                                        })
                                        && f["alpha"].as_f64().is_some_and(|n| {
                                            n.is_finite() && (0.0..=1.0).contains(&n)
                                        })
                                })
                        }))
                    && (g["graphs"].is_null()
                        || g["graphs"].as_array().is_some_and(|graphs| {
                            graphs.len() <= 16
                                && graphs.iter().all(|a| {
                                    ["rect", "plot", "bypass", "remove"]
                                        .iter()
                                        .all(|k| rect(&a[*k]))
                                        && a["slopes"]
                                            .as_array()
                                            .is_some_and(|s| s.len() == 2 && s.iter().all(rect))
                                        && numbers(&a["edges"], 2)
                                        && text(&a["title"])
                                        && a["slope_labels"]
                                            .as_array()
                                            .is_some_and(|s| s.len() == 2 && s.iter().all(text))
                                        && ["gain_y", "zero_y", "scale"].iter().all(|k| {
                                            a[*k]
                                                .as_f64()
                                                .is_some_and(|n| n.is_finite() && n.abs() <= 32768.)
                                        })
                                        && a["response"].as_array().is_some_and(|s| {
                                            s.len() <= 97 && s.iter().all(|p| numbers(p, 2))
                                        })
                                })
                        }))
            })
    })
}

pub(crate) fn label_text(text: &str, limit: usize) -> String {
    let mut result = String::new();
    for c in text.chars().filter(|c| !c.is_control()) {
        if result.len() + c.len_utf8() > limit {
            break;
        }
        result.push(c);
    }
    result
}
