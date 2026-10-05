//! Donut chart geometry as SVG path commands for Slint `Path` elements
//! (viewbox 100×100). Pure functions, unit tested.

const CENTER: f64 = 50.0;
const OUTER: f64 = 50.0;
/// Ring thickness of the design: 19px on a 166px donut.
const INNER: f64 = OUTER * (1.0 - 2.0 * 19.0 / 166.0);

fn point(radius: f64, fraction: f64) -> (f64, f64) {
    // Start at 12 o'clock, clockwise.
    let angle = fraction * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
    (CENTER + radius * angle.cos(), CENTER + radius * angle.sin())
}

fn arc_segment(start: f64, end: f64) -> String {
    let large = i32::from(end - start > 0.5);
    let (ox0, oy0) = point(OUTER, start);
    let (ox1, oy1) = point(OUTER, end);
    let (ix1, iy1) = point(INNER, end);
    let (ix0, iy0) = point(INNER, start);
    format!(
        "M {ox0:.3} {oy0:.3} A {OUTER} {OUTER} 0 {large} 1 {ox1:.3} {oy1:.3} L {ix1:.3} {iy1:.3} \
         A {INNER:.3} {INNER:.3} 0 {large} 0 {ix0:.3} {iy0:.3} Z"
    )
}

/// One path per positive value; a full ring is split in two arcs because a
/// single SVG arc cannot describe a closed circle.
pub fn segments(values: &[u64]) -> Vec<String> {
    let total: u64 = values.iter().sum();
    if total == 0 {
        return Vec::new();
    }
    let mut start = 0.0;
    let mut out = Vec::with_capacity(values.len());
    for &value in values {
        let fraction = value as f64 / total as f64;
        let end = start + fraction;
        out.push(if fraction >= 0.999_999 {
            format!("{} {}", arc_segment(0.0, 0.5), arc_segment(0.5, 1.0))
        } else if fraction > 0.0 {
            arc_segment(start, end)
        } else {
            String::new()
        });
        start = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cover_the_ring_in_order() {
        let paths = segments(&[52, 38, 24, 18, 10]);
        assert_eq!(paths.len(), 5);
        assert!(paths[0].starts_with("M 50.000 0.000"));
        assert!(segments(&[]).is_empty());
        assert!(segments(&[0, 0]).is_empty());
        assert!(segments(&[5])[0].matches('M').count() == 2, "full ring split in two");
    }
}
