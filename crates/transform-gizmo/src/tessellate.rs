//! Anti-aliased tessellator just for the shapes the gizmo draws.
//!
//! Only stroked paths and filled convex polygons are needed. Edges are
//! anti-aliased by "feathering": a fringe about one pixel wide that fades from
//! shape color to transparent, so no multisampling needed.
//! Coordinates are viewport space with y pointing down as bevy does.

use crate::color::Color32;
use crate::math::{rot90, Pos2, Vec2};

/// Points closer together than this in viewport unit space are treated as one.
const MIN_POINT_DISTANCE: f32 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vertex {
    pub(crate) pos: Pos2,
    pub(crate) color: Color32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Mesh {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) indices: Vec<u32>,
}

/// Path vertex and the vector it moves along to offset the path by one unit.
#[derive(Clone, Copy, Debug)]
struct PathPoint {
    pos: Pos2,
    normal: Vec2,
}

impl Mesh {
    /// Fills a convex polygon, with a feathered edge. Any winding style is fine.
    pub(crate) fn fill_convex(&mut self, points: &[Pos2], color: Color32, feathering: f32) {
        if color.a() == 0 {
            return;
        }

        let mut points = clean(points, true);
        if points.len() < 3 {
            return;
        }

        // Normals point outwards for one winding order.
        if signed_area(&points) < 0.0 {
            points.reverse();
        }

        let path = closed_path(&points);
        let half = feathering * 0.5;

        // Vertices alternate betwixt inner: solid, 2i and outer: transparent, 2i + 1.
        let base = self.next_index();
        for point in &path {
            self.vertex(point.pos - point.normal * half, color);
            self.vertex(point.pos + point.normal * half, Color32::TRANSPARENT);
        }

        let count = path.len() as u32;
        for i in 2..count {
            self.triangle(base, base + 2 * (i - 1), base + 2 * i);
        }
        for i in 0..count {
            let prev = (i + count - 1) % count;
            self.quad(
                base + 2 * prev,
                base + 2 * i,
                base + 2 * i + 1,
                base + 2 * prev + 1,
            );
        }
    }

    /// Stroke a path with butt ends and mitered joints with feathered edges.
    pub(crate) fn stroke_path(
        &mut self,
        points: &[Pos2],
        closed: bool,
        width: f32,
        color: Color32,
        feathering: f32,
    ) {
        if width <= 0.0 || color.a() == 0 {
            return;
        }

        let points = clean(points, closed);
        if points.len() < if closed { 3 } else { 2 } {
            return;
        }

        let path = if closed {
            closed_path(&points)
        } else {
            open_path(&points)
        };

        let rows: Vec<(f32, Color32)> = if width <= feathering {
            vec![
                (feathering, Color32::TRANSPARENT),
                (0.0, color.linear_multiply(width / feathering)),
                (-feathering, Color32::TRANSPARENT),
            ]
        } else {
            let outer = (width + feathering) * 0.5;
            let inner = (width - feathering) * 0.5;
            vec![
                (outer, Color32::TRANSPARENT),
                (inner, color),
                (-inner, color),
                (-outer, Color32::TRANSPARENT),
            ]
        };

        let base = self.next_index();
        for point in &path {
            for &(offset, row_color) in &rows {
                self.vertex(point.pos + point.normal * offset, row_color);
            }
        }

        let columns = rows.len() as u32;
        let count = path.len() as u32;
        let segments = if closed { count } else { count - 1 };
        for a in 0..segments {
            let b = (a + 1) % count;
            for k in 0..columns - 1 {
                self.quad(
                    base + a * columns + k,
                    base + b * columns + k,
                    base + b * columns + k + 1,
                    base + a * columns + k + 1,
                );
            }
        }
    }

    fn next_index(&self) -> u32 {
        self.vertices.len() as u32
    }

    fn vertex(&mut self, pos: Pos2, color: Color32) {
        self.vertices.push(Vertex { pos, color });
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }

    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.triangle(a, b, c);
        self.triangle(a, c, d);
    }
}

/// Drop non finite points and consecutive duplicates, which
/// produce zero length edges with no direction.
fn clean(points: &[Pos2], closed: bool) -> Vec<Pos2> {
    let mut out: Vec<Pos2> = Vec::with_capacity(points.len());

    for &point in points {
        if point.is_finite()
            && out
                .last()
                .is_none_or(|last| last.distance(point) > MIN_POINT_DISTANCE)
        {
            out.push(point);
        }
    }

    if closed
        && out.first().zip(out.last()).is_some_and(|(first, last)| {
            out.len() > 1 && first.distance(*last) <= MIN_POINT_DISTANCE
        })
    {
        out.pop();
    }

    out
}

/// Twice the signed area. Positive when normals from [`rot90`] face outwards.
fn signed_area(points: &[Pos2]) -> f32 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum()
}

fn edge_normal(from: Pos2, to: Pos2) -> Vec2 {
    rot90((to - from).normalize_or_zero())
}

fn open_path(points: &[Pos2]) -> Vec<PathPoint> {
    let mut path = Vec::with_capacity(points.len());

    let mut prev = edge_normal(points[0], points[1]);
    path.push(PathPoint {
        pos: points[0],
        normal: prev,
    });

    for window in points.windows(3) {
        let next = edge_normal(window[1], window[2]);
        add_corner(&mut path, window[1], prev, next);
        prev = next;
    }

    path.push(PathPoint {
        pos: points[points.len() - 1],
        normal: prev,
    });
    path
}

fn closed_path(points: &[Pos2]) -> Vec<PathPoint> {
    let mut path = Vec::with_capacity(points.len());
    let count = points.len();

    let mut prev = edge_normal(points[count - 1], points[0]);
    for (i, &pos) in points.iter().enumerate() {
        let next = edge_normal(pos, points[(i + 1) % count]);
        add_corner(&mut path, pos, prev, next);
        prev = next;
    }
    path
}

/// Adds the points for a corner between two edges with the given normals.
fn add_corner(path: &mut Vec<PathPoint>, pos: Pos2, prev: Vec2, next: Vec2) {
    let mut normal = (prev + next) * 0.5;
    let length_sq = normal.length_squared();

    if length_sq < 0.5 {
        // Sharper than a right angle, a miter would spike the output, so bevel it instead.
        let center = normal.normalize_or_zero();
        for edge in [prev, next] {
            let cut = (edge + center) * 0.5;
            let cut_length_sq = cut.length_squared();
            path.push(PathPoint {
                pos,
                normal: if cut_length_sq > f32::EPSILON {
                    cut / cut_length_sq
                } else {
                    edge
                },
            });
        }
    } else {
        normal /= length_sq;
        path.push(PathPoint { pos, normal });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Color32 = Color32::from_rgb(255, 0, 0);

    fn square() -> Vec<Pos2> {
        vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(0.0, 10.0),
        ]
    }

    fn assert_valid(mesh: &Mesh) {
        assert_eq!(mesh.indices.len() % 3, 0);
        let count = mesh.vertices.len() as u32;
        assert!(mesh.indices.iter().all(|&i| i < count));
        assert!(mesh.vertices.iter().all(|v| v.pos.is_finite()));
    }

    fn bounds(mesh: &Mesh, solid_only: bool) -> (Pos2, Pos2) {
        mesh.vertices
            .iter()
            .filter(|v| !solid_only || v.color.a() > 0)
            .fold((Pos2::MAX, Pos2::MIN), |(lo, hi), v| {
                (lo.min(v.pos), hi.max(v.pos))
            })
    }

    #[test]
    fn fill_square_counts_and_feather() {
        let mut mesh = Mesh::default();
        mesh.fill_convex(&square(), RED, 1.0);
        assert_valid(&mesh);

        // inner and outer per corner, a fan over the inner ring and a quad per edge
        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.indices.len(), (4 - 2) * 3 + 4 * 6);

        let (lo, hi) = bounds(&mesh, true);
        assert!((lo - Pos2::new(0.5, 0.5)).length() < 1e-4, "{lo:?}");
        assert!((hi - Pos2::new(9.5, 9.5)).length() < 1e-4, "{hi:?}");

        let (lo, hi) = bounds(&mesh, false);
        assert!((lo - Pos2::new(-0.5, -0.5)).length() < 1e-4, "{lo:?}");
        assert!((hi - Pos2::new(10.5, 10.5)).length() < 1e-4, "{hi:?}");
    }

    #[test]
    fn fill_ignores_winding() {
        let mut forward = Mesh::default();
        forward.fill_convex(&square(), RED, 1.0);

        let mut reversed = Mesh::default();
        let mut points = square();
        points.reverse();
        reversed.fill_convex(&points, RED, 1.0);

        assert_valid(&reversed);
        assert_eq!(forward.vertices.len(), reversed.vertices.len());
        assert_eq!(bounds(&forward, false), bounds(&reversed, false));
        assert_eq!(bounds(&forward, true), bounds(&reversed, true));
    }

    #[test]
    fn fill_drops_repeated_closing_point() {
        let mut points = square();
        points.push(points[0]);
        let mut mesh = Mesh::default();
        mesh.fill_convex(&points, RED, 1.0);
        assert_eq!(mesh.vertices.len(), 8);
    }

    #[test]
    fn fill_triangle_as_arrow_head() {
        let mut mesh = Mesh::default();
        mesh.fill_convex(
            &[
                Pos2::new(0.0, -2.0),
                Pos2::new(0.0, 2.0),
                Pos2::new(12.0, 0.0),
            ],
            RED,
            1.0,
        );
        assert_valid(&mesh);

        // every corner sharper than a right angle, so each splits in twain
        assert_eq!(mesh.vertices.len(), 3 * 2 * 2);

        // the solid portion must stay inside the triangle's bounds
        let (lo, hi) = bounds(&mesh, true);
        assert!(lo.x >= 0.0 && lo.y >= -2.0 && hi.x <= 12.0 && hi.y <= 2.0);
    }

    #[test]
    fn stroke_thick_line_profile() {
        let mut mesh = Mesh::default();
        mesh.stroke_path(
            &[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)],
            false,
            4.0,
            RED,
            1.0,
        );
        assert_valid(&mesh);

        assert_eq!(mesh.vertices.len(), 8);
        assert_eq!(mesh.indices.len(), 3 * 6);

        let (lo, hi) = bounds(&mesh, true);
        assert!((lo.y + 1.5).abs() < 1e-4 && (hi.y - 1.5).abs() < 1e-4);
        let (lo, hi) = bounds(&mesh, false);
        assert!((lo.y + 2.5).abs() < 1e-4 && (hi.y - 2.5).abs() < 1e-4);
        // butt caps do not grow past end points
        assert!(lo.x.abs() < 1e-4 && (hi.x - 10.0).abs() < 1e-4);
    }

    #[test]
    fn stroke_thin_line_fades_instead_of_shrinking() {
        let mut mesh = Mesh::default();
        mesh.stroke_path(
            &[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)],
            false,
            0.25,
            RED,
            1.0,
        );
        assert_valid(&mesh);
        assert_eq!(mesh.vertices.len(), 6);

        let center = mesh.vertices[1].color;
        assert!(center.a() > 0 && center.a() < 255, "{center:?}");
    }

    #[test]
    fn stroke_closed_square() {
        let mut mesh = Mesh::default();
        mesh.stroke_path(&square(), true, 2.0, RED, 1.0);
        assert_valid(&mesh);

        // right angles keep a single miter per corner
        assert_eq!(mesh.vertices.len(), 4 * 4);
        assert_eq!(mesh.indices.len(), 4 * 3 * 6);

        let (lo, hi) = bounds(&mesh, false);
        assert!((lo - Pos2::new(-1.5, -1.5)).length() < 1e-4, "{lo:?}");
        assert!((hi - Pos2::new(11.5, 11.5)).length() < 1e-4, "{hi:?}");
    }

    #[test]
    fn sharp_corner_is_beveled_not_spiked() {
        let mut mesh = Mesh::default();
        mesh.stroke_path(
            &[
                Pos2::new(0.0, 0.0),
                Pos2::new(10.0, 0.0),
                Pos2::new(0.0, 0.5),
            ],
            false,
            2.0,
            RED,
            1.0,
        );
        assert_valid(&mesh);

        // two end points plus a corner split in twain, four vertices each
        assert_eq!(mesh.vertices.len(), 4 * 4);

        // Unbeveled miter reaches past the 10 unit corner
        let (_, hi) = bounds(&mesh, false);
        assert!(hi.x < 13.0, "{hi:?}");
    }

    #[test]
    fn reversal_stays_finite() {
        let mut mesh = Mesh::default();
        mesh.stroke_path(
            &[
                Pos2::new(0.0, 0.0),
                Pos2::new(10.0, 0.0),
                Pos2::new(0.0, 0.0),
            ],
            false,
            2.0,
            RED,
            1.0,
        );
        assert_valid(&mesh);
        assert!(!mesh.vertices.is_empty());
    }

    #[test]
    fn degenerate_input_makes_nothing() {
        let mut mesh = Mesh::default();
        let p = Pos2::new(3.0, 3.0);

        mesh.stroke_path(&[], false, 2.0, RED, 1.0);
        mesh.stroke_path(&[p], false, 2.0, RED, 1.0);
        mesh.stroke_path(&[p, p, p], false, 2.0, RED, 1.0);
        mesh.stroke_path(&square(), true, 0.0, RED, 1.0);
        mesh.stroke_path(&square(), true, 2.0, Color32::TRANSPARENT, 1.0);
        mesh.stroke_path(&[p, Pos2::new(f32::NAN, 0.0)], false, 2.0, RED, 1.0);
        mesh.fill_convex(&[p, Pos2::new(9.0, 9.0)], RED, 1.0);
        mesh.fill_convex(&[p, p, p, p], RED, 1.0);
        mesh.fill_convex(&square(), Color32::TRANSPARENT, 1.0);

        assert!(mesh.vertices.is_empty() && mesh.indices.is_empty());
    }

    #[test]
    fn collinear_fill_stays_finite() {
        let mut mesh = Mesh::default();
        mesh.fill_convex(
            &[
                Pos2::new(0.0, 0.0),
                Pos2::new(5.0, 0.0),
                Pos2::new(10.0, 0.0),
            ],
            RED,
            1.0,
        );
        assert_valid(&mesh);
    }

    #[test]
    fn feathering_scales_with_density() {
        let mut mesh = Mesh::default();
        mesh.fill_convex(&square(), RED, 0.5);
        let (lo, _) = bounds(&mesh, false);
        assert!((lo.x + 0.25).abs() < 1e-4, "{lo:?}");
    }
}
