//! A minimal sRGB color type for gizmo visuals and vertex colors.

/// Gamma sRGB color with premultiplied alpha, one byte per channel.
///
/// Premultiplied alpha is what the gizmo renderers blend with, so that draw data
/// needs no conversion step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Color32([u8; 4]);

impl Default for Color32 {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

impl Color32 {
    /// Fully transparent, all channels zero.
    pub const TRANSPARENT: Self = Self([0, 0, 0, 0]);
    pub const BLACK: Self = Self([0, 0, 0, 255]);
    pub const WHITE: Self = Self([255, 255, 255, 255]);

    /// Opaque color from gamma encoded channels.
    #[must_use]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }

    /// Color from channels that are already multiplied by alpha.
    #[must_use]
    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    /// Color from gamma encoded channels that are not multiplied by alpha yet.
    ///
    /// Multiplication happens in linear space, which keeps mid alpha values
    /// perceptually correct.
    #[must_use]
    pub fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        match a {
            255 => Self::from_rgb(r, g, b),
            0 => Self::TRANSPARENT,
            _ => {
                let alpha = f32::from(a) / 255.0;
                let premultiply = |c: u8| gamma_from_linear(linear_from_gamma(c) * alpha);
                Self([premultiply(r), premultiply(g), premultiply(b), a])
            }
        }
    }

    /// Red channel, premultiplied.
    #[must_use]
    pub const fn r(self) -> u8 {
        self.0[0]
    }

    /// Green channel, premultiplied.
    #[must_use]
    pub const fn g(self) -> u8 {
        self.0[1]
    }

    /// Blue channel, premultiplied.
    #[must_use]
    pub const fn b(self) -> u8 {
        self.0[2]
    }

    /// Alpha channel.
    #[must_use]
    pub const fn a(self) -> u8 {
        self.0[3]
    }

    /// The premultiplied channels as stored.
    #[must_use]
    pub const fn to_array(self) -> [u8; 4] {
        self.0
    }

    /// Scale every channel by `factor` in gamma space.
    ///
    /// Cheap, and what you want for fading a color against a dark background.
    #[must_use]
    pub fn gamma_multiply(self, factor: f32) -> Self {
        Self(self.0.map(|c| scale_byte(c, factor)))
    }

    /// Scale every channel by `factor` in linear space.
    ///
    /// Slower than [`Self::gamma_multiply`], but matches how the blend hardware
    /// treats the resulting alpha.
    #[must_use]
    pub fn linear_multiply(self, factor: f32) -> Self {
        let [r, g, b, a] = self.to_linear_premultiplied();
        Self::from_linear_premultiplied([r * factor, g * factor, b * factor, a * factor])
    }

    /// Linear, premultiplied RGBA.
    #[must_use]
    pub fn to_linear_premultiplied(self) -> [f32; 4] {
        let [r, g, b, a] = self.0;
        [
            linear_from_gamma(r),
            linear_from_gamma(g),
            linear_from_gamma(b),
            f32::from(a) / 255.0,
        ]
    }

    fn from_linear_premultiplied([r, g, b, a]: [f32; 4]) -> Self {
        Self([
            gamma_from_linear(r),
            gamma_from_linear(g),
            gamma_from_linear(b),
            scale_byte(255, a),
        ])
    }
}

/// Multiplies byte by a factor, rounds and saturates into `0..=255`.
fn scale_byte(value: u8, factor: f32) -> u8 {
    // NB: `as` saturates for floats.
    (f32::from(value) * factor + 0.5) as u8
}

/// Standard sRGB conversion for a single byte.
fn linear_from_gamma(value: u8) -> f32 {
    let v = f32::from(value) / 255.0;
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse of [`linear_from_gamma`].
fn gamma_from_linear(linear: f32) -> u8 {
    let v = linear.clamp(0.0, 1.0);
    let encoded = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    scale_byte(255, encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gamma_round_trips_every_byte() {
        for value in 0..=255u8 {
            assert_eq!(gamma_from_linear(linear_from_gamma(value)), value);
        }
    }

    #[test]
    fn opaque_white_is_linear_one() {
        assert_eq!(Color32::WHITE.to_linear_premultiplied(), [1.0; 4]);
        assert_eq!(Color32::TRANSPARENT.to_linear_premultiplied(), [0.0; 4]);
    }

    #[test]
    fn multiply_by_zero_is_transparent() {
        let color = Color32::from_rgb(255, 0, 125);
        assert_eq!(color.gamma_multiply(0.0), Color32::TRANSPARENT);
        assert_eq!(color.linear_multiply(0.0), Color32::TRANSPARENT);
    }

    #[test]
    fn multiply_by_one_is_identity() {
        let color = Color32::from_rgb(255, 0, 125);
        assert_eq!(color.gamma_multiply(1.0), color);
        assert_eq!(color.linear_multiply(1.0), color);
    }

    #[test]
    fn linear_multiply_halves_linear_alpha() {
        let half = Color32::WHITE.linear_multiply(0.5);
        assert_eq!(half.a(), 128);
        let [r, _, _, a] = half.to_linear_premultiplied();
        assert!((r - 0.5).abs() < 0.01 && (a - 0.5).abs() < 0.01);
    }

    #[test]
    fn unmultiplied_edges() {
        assert_eq!(
            Color32::from_rgba_unmultiplied(10, 20, 30, 255),
            Color32::from_rgb(10, 20, 30)
        );
        assert_eq!(
            Color32::from_rgba_unmultiplied(10, 20, 30, 0),
            Color32::TRANSPARENT
        );
        let mid = Color32::from_rgba_unmultiplied(255, 255, 255, 128);
        assert_eq!(mid.a(), 128);
        assert!(mid.r() < 255);
    }

    #[test]
    fn default_is_transparent() {
        assert_eq!(Color32::default(), Color32::TRANSPARENT);
    }
}
