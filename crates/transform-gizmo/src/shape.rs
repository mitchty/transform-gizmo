use std::f64::consts::TAU;

use crate::color::Color32;
use crate::math::{Pos2, Rect, rot90};
use crate::tessellate::Mesh;
use glam::{DMat4, DVec3};

use crate::math::world_to_screen;

const STEPS_PER_RAD: f64 = 20.0;

/// Width and color of a stroked line.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Stroke {
    pub(crate) width: f32,
    pub(crate) color: Color32,
}

impl From<(f32, Color32)> for Stroke {
    fn from((width, color): (f32, Color32)) -> Self {
        Self { width, color }
    }
}

pub(crate) struct ShapeBuilder {
    mvp: DMat4,
    viewport: Rect,
    /// Width of the anti-aliasing fringe, one physical pixel in viewport units.
    feathering: f32,
}

impl ShapeBuilder {
    pub(crate) fn new(mvp: DMat4, viewport: Rect, pixels_per_point: f32) -> Self {
        Self {
            mvp,
            viewport,
            feathering: 1.0 / pixels_per_point.max(1e-3),
        }
    }

    fn stroke_path(&self, points: &[Pos2], closed: bool, stroke: Stroke) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.stroke_path(points, closed, stroke.width, stroke.color, self.feathering);
        mesh
    }

    fn fill(&self, points: &[Pos2], color: Color32) -> Mesh {
        let mut mesh = Mesh::default();
        mesh.fill_convex(points, color, self.feathering);
        mesh
    }

    fn arc_points(&self, radius: f64, start_angle: f64, end_angle: f64) -> Vec<Pos2> {
        let angle = f64::clamp(end_angle - start_angle, -TAU, TAU);

        let step_count = steps(angle);
        let mut points = Vec::with_capacity(step_count);

        let step_size = angle / (step_count - 1) as f64;

        for step in (0..step_count).map(|i| step_size * i as f64) {
            let x = f64::cos(start_angle + step) * radius;
            let z = f64::sin(start_angle + step) * radius;

            points.push(DVec3::new(x, 0.0, z));
        }

        points
            .into_iter()
            .filter_map(|point| self.vec3_to_pos2(point))
            .collect::<Vec<_>>()
    }

    pub(crate) fn arc(
        &self,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        stroke: impl Into<Stroke>,
    ) -> Mesh {
        let mut points = self.arc_points(radius, start_angle, end_angle);

        let closed = points
            .first()
            .zip(points.last())
            .filter(|(first, last)| first.distance(**last) < 1e-2)
            .is_some();

        if closed {
            points.pop();
        }

        self.stroke_path(&points, closed, stroke.into())
    }

    pub(crate) fn circle(&self, radius: f64, stroke: impl Into<Stroke>) -> Mesh {
        self.arc(radius, 0.0, TAU, stroke)
    }

    pub(crate) fn filled_circle(&self, radius: f64, color: Color32) -> Mesh {
        let mut points = self.arc_points(radius, 0.0, TAU);
        points.pop();

        self.fill(&points, color)
    }

    pub(crate) fn line_segment(&self, from: DVec3, to: DVec3, stroke: impl Into<Stroke>) -> Mesh {
        let (Some(from), Some(to)) = (
            world_to_screen(self.viewport, self.mvp, from),
            world_to_screen(self.viewport, self.mvp, to),
        ) else {
            return Mesh::default();
        };

        self.stroke_path(&[from, to], false, stroke.into())
    }

    pub(crate) fn arrow(&self, from: DVec3, to: DVec3, stroke: impl Into<Stroke>) -> Mesh {
        let stroke = stroke.into();
        let arrow_start = world_to_screen(self.viewport, self.mvp, from);
        let arrow_end = world_to_screen(self.viewport, self.mvp, to);

        let Some((start, end)) = arrow_start.zip(arrow_end) else {
            return Mesh::default();
        };

        let cross = rot90((end - start).normalize_or_zero()) * stroke.width / 2.0;

        self.fill(&[start - cross, start + cross, end], stroke.color)
    }

    pub(crate) fn polygon(&self, points: &[DVec3], fill: Color32) -> Mesh {
        let points = points
            .iter()
            .filter_map(|pos| world_to_screen(self.viewport, self.mvp, *pos))
            .collect::<Vec<_>>();

        self.fill(&points, fill)
    }

    pub(crate) fn polyline(&self, points: &[DVec3], stroke: impl Into<Stroke>) -> Mesh {
        let points = points
            .iter()
            .filter_map(|pos| world_to_screen(self.viewport, self.mvp, *pos))
            .collect::<Vec<_>>();

        self.stroke_path(&points, false, stroke.into())
    }

    pub(crate) fn sector(
        &self,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
        fill: Color32,
    ) -> Mesh {
        let angle_delta = end_angle - start_angle;
        let step_count = steps(angle_delta.abs());

        if step_count < 2 {
            return Mesh::default();
        }

        let mut points = Vec::with_capacity(step_count + 1);

        let step_size = angle_delta / (step_count - 1) as f64;

        if ((start_angle - end_angle).abs() - TAU).abs() < step_size.abs() {
            return self.filled_circle(radius, fill);
        }

        points.push(DVec3::new(0.0, 0.0, 0.0));

        let (sin_step, cos_step) = step_size.sin_cos();
        let (mut sin_angle, mut cos_angle) = start_angle.sin_cos();

        for _ in 0..step_count {
            let x = cos_angle * radius;
            let z = sin_angle * radius;

            points.push(DVec3::new(x, 0.0, z));

            let new_sin = sin_angle * cos_step + cos_angle * sin_step;
            let new_cos = cos_angle * cos_step - sin_angle * sin_step;

            sin_angle = new_sin;
            cos_angle = new_cos;
        }

        let points = points
            .into_iter()
            .filter_map(|point| self.vec3_to_pos2(point))
            .collect::<Vec<_>>();

        self.fill(&points, fill)
    }

    fn vec3_to_pos2(&self, vec: DVec3) -> Option<Pos2> {
        world_to_screen(self.viewport, self.mvp, vec)
    }
}

fn steps(angle: f64) -> usize {
    (STEPS_PER_RAD * angle.abs()).ceil().max(1.0) as usize
}
