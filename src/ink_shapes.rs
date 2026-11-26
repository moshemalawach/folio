//! Ink to Shape: recognises a hand-drawn stroke as a straight line or a simple closed
//! shape, the way the desktop notebook tidies drawings into shapes as you make them.

use eframe::egui::{pos2, Pos2, Rect};

use crate::model::ShapeKind;

/// Strokes smaller than this (in page points) stay ink: dots, ticks, handwriting.
const MIN_SIZE: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecognizedShape {
    pub kind: ShapeKind,
    /// The shape's box on the page. A line's box spans its two ends and may be 0 thick.
    pub rect: Rect,
    /// For lines, which corner each end sits at, as in `CanvasKind::Shape`. For a
    /// triangle, `flip_y` means it points down.
    pub flip_x: bool,
    pub flip_y: bool,
}

/// The shape a stroke looks like, or `None` to keep it as ink.
pub fn recognize_shape(points: &[[f32; 2]]) -> Option<RecognizedShape> {
    let mut path: Vec<Pos2> = Vec::with_capacity(points.len());
    for point in points {
        let point = pos2(point[0], point[1]);
        if !point.x.is_finite() || !point.y.is_finite() {
            return None;
        }
        if path.last() != Some(&point) {
            path.push(point);
        }
    }
    if path.len() < 3 {
        return None;
    }
    let bounds = Rect::from_points(&path);
    let size = bounds.width().max(bounds.height());
    if size < MIN_SIZE {
        return None;
    }
    let (first, last) = (path[0], path[path.len() - 1]);
    let length: f32 = path.windows(2).map(|pair| pair[0].distance(pair[1])).sum();
    if let Some(line) = straight_line(&path, first, last, length) {
        return Some(line);
    }

    // Anything else must come back to where it started; open curves stay ink.
    let (width, height) = (bounds.width(), bounds.height());
    let short = width.min(height);
    if path.len() < 8
        || first.distance(last) > size * 0.25
        || length < size * 2.0
        || short < size * 0.15
    {
        return None;
    }
    let closed = |kind, flip_y| {
        Some(RecognizedShape {
            kind,
            rect: bounds,
            flip_x: false,
            flip_y,
        })
    };
    let corners = [
        bounds.left_top(),
        bounds.right_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ];
    let ring = simplify_loop(&path, size * 0.1);
    // How far the stroke strays from an ellipse filling its box, relative to the radius.
    let center = bounds.center();
    let ellipse_fit = path
        .iter()
        .map(|p| {
            let dx = (p.x - center.x) / (width / 2.0);
            let dy = (p.y - center.y) / (height / 2.0);
            ((dx * dx + dy * dy).sqrt() - 1.0).abs()
        })
        .sum::<f32>()
        / path.len() as f32;
    // How far the stroke strays from its own corners joined up, on the same scale.
    let polygon_fit = path
        .iter()
        .map(|p| distance_to_ring(*p, &ring))
        .sum::<f32>()
        / path.len() as f32
        / (short / 2.0);
    if let Some((kind, flip_y)) = polygon_kind(&ring, bounds, corners) {
        if polygon_fit <= ellipse_fit && polygon_fit < 0.15 {
            return closed(kind, flip_y);
        }
    }
    // Ellipses keep away from their box's corners; rectangles reach into them.
    let corner_gap = corners
        .iter()
        .map(|corner| {
            path.iter()
                .map(|p| p.distance(*corner))
                .fold(f32::INFINITY, f32::min)
        })
        .sum::<f32>()
        / 4.0
        / short;
    // A pentagon or hexagon is round enough to pass for an ellipse, but its sides are
    // straighter than any hand-drawn circle's; there is no such shape, so it stays ink.
    // (A circle simplifies to 5 to 9 corners too, but strays from them far more.)
    let polygonal = matches!(ring.len(), 5 | 6) && polygon_fit < ellipse_fit * 0.6;
    if ellipse_fit < 0.12 && corner_gap > 0.15 && !polygonal {
        return closed(ShapeKind::Ellipse, false);
    }
    let edge_fit = path
        .iter()
        .map(|p| {
            (p.x - bounds.left())
                .abs()
                .min((p.x - bounds.right()).abs())
                .min((p.y - bounds.top()).abs())
                .min((p.y - bounds.bottom()).abs())
        })
        .sum::<f32>()
        / path.len() as f32
        / short;
    if edge_fit < 0.06 && corner_gap < 0.12 {
        return closed(ShapeKind::Rect, false);
    }
    None
}

fn straight_line(path: &[Pos2], first: Pos2, last: Pos2, length: f32) -> Option<RecognizedShape> {
    let chord = first.distance(last);
    if chord < MIN_SIZE || chord < length * 0.92 {
        return None;
    }
    let wobble = path
        .iter()
        .map(|p| distance_to_line(*p, first, last))
        .fold(0.0, f32::max);
    if wobble > (chord * 0.08).max(4.0) {
        return None;
    }
    // Nearly level or upright lines snap straight.
    let mut end = last;
    if (last.y - first.y).abs() < chord * 0.09 {
        end.y = first.y;
    } else if (last.x - first.x).abs() < chord * 0.09 {
        end.x = first.x;
    }
    Some(RecognizedShape {
        kind: ShapeKind::Line,
        rect: Rect::from_two_pos(first, end),
        flip_x: end.x < first.x,
        flip_y: end.y < first.y,
    })
}

/// A triangle, rectangle or diamond whose corners sit where that shape's corners would.
/// The triangle shape points straight up or down, so a right-angled or sideways triangle
/// stays ink rather than turning into a different triangle.
fn polygon_kind(ring: &[Pos2], bounds: Rect, corners: [Pos2; 4]) -> Option<(ShapeKind, bool)> {
    let (width, height) = (bounds.width(), bounds.height());
    let midpoints = [
        bounds.center_top(),
        bounds.right_center(),
        bounds.center_bottom(),
        bounds.left_center(),
    ];
    let up = [
        bounds.center_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ];
    let down = [
        bounds.center_bottom(),
        bounds.left_top(),
        bounds.right_top(),
    ];
    let tolerance = width.max(height) * 0.2;
    match ring.len() {
        3 if near_all(ring, &up, tolerance) => Some((ShapeKind::Triangle, false)),
        3 if near_all(ring, &down, tolerance) => Some((ShapeKind::Triangle, true)),
        4 if near_all(ring, &corners, width.min(height) * 0.25) => Some((ShapeKind::Rect, false)),
        4 if near_all(ring, &midpoints, width.max(height) * 0.2) => {
            Some((ShapeKind::Diamond, false))
        }
        _ => None,
    }
}

fn near_all(points: &[Pos2], targets: &[Pos2], tolerance: f32) -> bool {
    targets.iter().all(|target| {
        points
            .iter()
            .any(|point| point.distance(*target) <= tolerance)
    })
}

/// The corners of a closed stroke: Douglas–Peucker around the loop, then corners that
/// lie on a straight run (such as where the pen started) are merged away.
fn simplify_loop(path: &[Pos2], epsilon: f32) -> Vec<Pos2> {
    let start = path[0];
    let far = (0..path.len())
        .max_by(|&a, &b| path[a].distance(start).total_cmp(&path[b].distance(start)))
        .unwrap_or(0)
        .max(1);
    let mut ring = douglas_peucker(&path[..=far], epsilon);
    let mut back = path[far..].to_vec();
    back.push(start);
    let back = douglas_peucker(&back, epsilon);
    ring.pop(); // `far` also starts the second half…
    ring.extend_from_slice(&back[..back.len() - 1]); // …and `start` also ends it.
    while ring.len() > 3 {
        let n = ring.len();
        let flat = (0..n).find(|&i| {
            let (prev, here, next) = (ring[(i + n - 1) % n], ring[i], ring[(i + 1) % n]);
            distance_to_segment(here, prev, next) < epsilon || here.distance(next) < epsilon
        });
        match flat {
            Some(i) => {
                ring.remove(i);
            }
            None => break,
        }
    }
    ring
}

fn douglas_peucker(points: &[Pos2], epsilon: f32) -> Vec<Pos2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let (index, distance) = points[1..points.len() - 1]
        .iter()
        .enumerate()
        .map(|(i, p)| (i + 1, distance_to_segment(*p, first, last)))
        .fold(
            (0, 0.0),
            |best, item| if item.1 > best.1 { item } else { best },
        );
    if distance <= epsilon {
        return vec![first, last];
    }
    let mut left = douglas_peucker(&points[..=index], epsilon);
    let right = douglas_peucker(&points[index..], epsilon);
    left.pop();
    left.extend(right);
    left
}

fn distance_to_ring(point: Pos2, ring: &[Pos2]) -> f32 {
    (0..ring.len())
        .map(|i| distance_to_segment(point, ring[i], ring[(i + 1) % ring.len()]))
        .fold(f32::INFINITY, f32::min)
}

fn distance_to_segment(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let length = ab.length_sq();
    if length <= f32::EPSILON {
        return point.distance(a);
    }
    let t = ((point - a).dot(ab) / length).clamp(0.0, 1.0);
    point.distance(a + ab * t)
}

fn distance_to_line(point: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let length = ab.length();
    if length <= f32::EPSILON {
        return point.distance(a);
    }
    let ap = point - a;
    (ap.x * ab.y - ap.y * ab.x).abs() / length
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A repeatable wobble in `-amount..=amount`, like a hand on a trackpad.
    fn wobble(i: usize, amount: f32) -> f32 {
        (((i * 7919 + 13) % 17) as f32 / 8.0 - 1.0) * amount
    }

    /// Walks through `corners` in order, `steps` points per side.
    fn trace(corners: &[[f32; 2]], steps: usize, amount: f32) -> Vec<[f32; 2]> {
        let mut points = Vec::new();
        for (side, pair) in corners.windows(2).enumerate() {
            for step in 0..steps {
                let t = step as f32 / steps as f32;
                let i = side * steps + step;
                points.push([
                    pair[0][0] + (pair[1][0] - pair[0][0]) * t + wobble(i, amount),
                    pair[0][1] + (pair[1][1] - pair[0][1]) * t + wobble(i + 5, amount),
                ]);
            }
        }
        points.push(*corners.last().unwrap());
        points
    }

    fn oval(
        center: [f32; 2],
        rx: f32,
        ry: f32,
        turns: f32,
        steps: usize,
        amount: f32,
    ) -> Vec<[f32; 2]> {
        (0..=steps)
            .map(|i| {
                let a = i as f32 / steps as f32 * turns * std::f32::consts::TAU;
                [
                    center[0] + rx * a.cos() + wobble(i, amount),
                    center[1] + ry * a.sin() + wobble(i + 3, amount),
                ]
            })
            .collect()
    }

    fn kind(points: &[[f32; 2]]) -> Option<ShapeKind> {
        recognize_shape(points).map(|shape| shape.kind)
    }

    #[test]
    fn straight_strokes_become_lines() {
        let level = recognize_shape(&trace(&[[0.0, 100.0], [200.0, 104.0]], 12, 1.5)).unwrap();
        assert_eq!(level.kind, ShapeKind::Line);
        assert_eq!(level.rect.height(), 0.0, "nearly level lines snap level");
        let slope = recognize_shape(&trace(&[[0.0, 0.0], [150.0, 120.0]], 12, 1.5)).unwrap();
        assert_eq!(
            (slope.kind, slope.flip_x, slope.flip_y),
            (ShapeKind::Line, false, false)
        );
        let back = recognize_shape(&trace(&[[150.0, 120.0], [0.0, 0.0]], 12, 1.5)).unwrap();
        assert!(
            back.flip_x && back.flip_y,
            "a line keeps the direction it was drawn in"
        );
    }

    #[test]
    fn rough_rectangles_become_rectangles() {
        let shape = recognize_shape(&trace(
            &[
                [0.0, 0.0],
                [200.0, 0.0],
                [200.0, 120.0],
                [0.0, 120.0],
                [4.0, 6.0],
            ],
            10,
            2.5,
        ))
        .unwrap();
        assert_eq!(shape.kind, ShapeKind::Rect);
        assert!((shape.rect.width() - 200.0).abs() < 8.0);
        assert!((shape.rect.height() - 120.0).abs() < 8.0);
        // Starting halfway along a side makes no difference.
        let midway = trace(
            &[
                [100.0, 0.0],
                [200.0, 0.0],
                [200.0, 120.0],
                [0.0, 120.0],
                [0.0, 0.0],
                [96.0, 3.0],
            ],
            10,
            2.5,
        );
        assert_eq!(kind(&midway), Some(ShapeKind::Rect));
    }

    #[test]
    fn circles_and_ovals_become_ellipses() {
        assert_eq!(
            kind(&oval([100.0, 100.0], 60.0, 60.0, 1.04, 40, 2.5)),
            Some(ShapeKind::Ellipse)
        );
        assert_eq!(
            kind(&oval([100.0, 100.0], 90.0, 45.0, 1.0, 48, 2.0)),
            Some(ShapeKind::Ellipse)
        );
    }

    #[test]
    fn triangles_and_diamonds() {
        let up = recognize_shape(&trace(
            &[[100.0, 0.0], [200.0, 160.0], [0.0, 160.0], [97.0, 5.0]],
            10,
            2.0,
        ))
        .unwrap();
        assert_eq!((up.kind, up.flip_y), (ShapeKind::Triangle, false));
        let down = recognize_shape(&trace(
            &[[0.0, 0.0], [200.0, 0.0], [100.0, 160.0], [4.0, 4.0]],
            10,
            2.0,
        ))
        .unwrap();
        assert_eq!((down.kind, down.flip_y), (ShapeKind::Triangle, true));
        let diamond = trace(
            &[
                [100.0, 0.0],
                [200.0, 80.0],
                [100.0, 160.0],
                [0.0, 80.0],
                [96.0, 5.0],
            ],
            10,
            2.0,
        );
        assert_eq!(kind(&diamond), Some(ShapeKind::Diamond));
        // A slightly skewed triangle still tidies into the upright one.
        let skewed = trace(
            &[[130.0, 0.0], [200.0, 160.0], [0.0, 160.0], [127.0, 5.0]],
            10,
            2.0,
        );
        assert_eq!(kind(&skewed), Some(ShapeKind::Triangle));
    }

    #[test]
    fn triangles_that_point_elsewhere_stay_ink() {
        // The triangle shape only points up or down: anything else would change shape.
        for (name, corners) in [
            ("◺", [[0.0, 0.0], [0.0, 160.0], [200.0, 160.0], [4.0, 4.0]]),
            ("◸", [[0.0, 0.0], [200.0, 0.0], [0.0, 160.0], [3.0, 4.0]]),
            (
                "◿",
                [[200.0, 0.0], [200.0, 160.0], [0.0, 160.0], [196.0, 4.0]],
            ),
            ("▶", [[0.0, 0.0], [200.0, 80.0], [0.0, 160.0], [2.0, 5.0]]),
            (
                "◀",
                [[200.0, 0.0], [0.0, 80.0], [200.0, 160.0], [197.0, 5.0]],
            ),
        ] {
            assert_eq!(kind(&trace(&corners, 10, 2.0)), None, "{name}");
        }
    }

    #[test]
    fn pentagons_and_hexagons_stay_ink() {
        for sides in [5usize, 6] {
            for turn in [0.0f32, 0.3, -std::f32::consts::FRAC_PI_2] {
                for amount in [0.0f32, 2.0, 4.0] {
                    let mut corners: Vec<[f32; 2]> = (0..=sides)
                        .map(|i| {
                            let a = turn + i as f32 / sides as f32 * std::f32::consts::TAU;
                            [200.0 + 100.0 * a.cos(), 200.0 + 100.0 * a.sin()]
                        })
                        .collect();
                    corners[sides][0] += 3.0;
                    corners[sides][1] += 2.0;
                    assert_eq!(
                        kind(&trace(&corners, 10, amount)),
                        None,
                        "{sides} sides, turned {turn}, wobble {amount}"
                    );
                }
            }
        }
        // Circles and ovals drawn with a shaky hand, starting anywhere, still count.
        for (rx, ry) in [(60.0, 60.0), (100.0, 100.0), (90.0, 45.0), (40.0, 80.0)] {
            for start in [0.0f32, 0.7, 2.0] {
                for (steps, amount) in [(40, 2.5), (24, 1.0), (80, 4.0), (60, 0.0)] {
                    let points: Vec<[f32; 2]> = (0..=steps)
                        .map(|i| {
                            let a = start + i as f32 / steps as f32 * std::f32::consts::TAU;
                            [
                                200.0 + rx * a.cos() + wobble(i, amount),
                                200.0 + ry * a.sin() + wobble(i + 3, amount),
                            ]
                        })
                        .collect();
                    assert_eq!(
                        kind(&points),
                        Some(ShapeKind::Ellipse),
                        "{rx}x{ry} from {start}, {steps} points, wobble {amount}"
                    );
                }
            }
        }
    }

    #[test]
    fn scribbles_and_open_curves_stay_ink() {
        let zigzag: Vec<[f32; 2]> = (0..20)
            .map(|i| [i as f32 * 10.0, if i % 2 == 0 { 0.0 } else { 40.0 }])
            .collect();
        assert_eq!(kind(&zigzag), None, "a zigzag");
        let spiral: Vec<[f32; 2]> = (0..120)
            .map(|i| {
                let (a, r) = (i as f32 * 0.16, 10.0 + i as f32 * 0.6);
                [100.0 + r * a.cos(), 100.0 + r * a.sin()]
            })
            .collect();
        assert_eq!(kind(&spiral), None, "a spiral");
        assert_eq!(
            kind(&oval([100.0, 100.0], 60.0, 60.0, 0.5, 24, 1.0)),
            None,
            "an open arc"
        );
        assert_eq!(
            kind(&oval([10.0, 10.0], 4.0, 4.0, 1.0, 16, 0.0)),
            None,
            "too small"
        );
        let eight: Vec<[f32; 2]> = (0..=64)
            .map(|i| {
                let a = i as f32 / 64.0 * std::f32::consts::TAU;
                [100.0 + 100.0 * a.sin(), 100.0 + 50.0 * a.sin() * a.cos()]
            })
            .collect();
        assert_eq!(kind(&eight), None, "a figure of eight");
        assert_eq!(
            kind(&trace(&[[0.0, 50.0], [30.0, 90.0], [110.0, 0.0]], 8, 1.0)),
            None,
            "a tick"
        );
    }
}
