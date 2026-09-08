//! A minimal 3D vector, so this crate takes no dependency for eight lines of
//! arithmetic.
//!
//! Coordinate system, fixed once in docs/design/0003-rig-model.md and never
//! re-litigated: right-handed, **origin at the eye point**, `+X` right, `+Y`
//! up, `+Z` forward toward the screens.
//!
//! Putting the origin at the eye is what keeps the rest of this crate short: a
//! screen corner's position *is* its direction vector, so an angle is one
//! `atan2` rather than a subtraction and a normalisation.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    pub fn normalised(self) -> Vec3 {
        let l = self.length();
        if l == 0.0 {
            self
        } else {
            self * (1.0 / l)
        }
    }

    pub fn dot(self, o: Vec3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    /// Rotate about the vertical axis. Positive yaw turns `+Z` toward `+X`.
    pub fn yaw(self, radians: f64) -> Vec3 {
        let (s, c) = radians.sin_cos();
        Vec3::new(self.x * c + self.z * s, self.y, -self.x * s + self.z * c)
    }

    /// Rotate about the lateral axis. Positive pitch tips `+Z` toward `+Y`.
    pub fn pitch(self, radians: f64) -> Vec3 {
        let (s, c) = radians.sin_cos();
        Vec3::new(self.x, self.y * c - self.z * s, self.y * s + self.z * c)
    }

    /// Rotate about the forward axis.
    pub fn roll(self, radians: f64) -> Vec3 {
        let (s, c) = radians.sin_cos();
        Vec3::new(self.x * c - self.y * s, self.x * s + self.y * c, self.z)
    }

    /// Horizontal angle from straight ahead, in degrees. Positive is right.
    ///
    /// Measured in the XZ plane, so a point's height does not change its
    /// horizontal bearing — which is what a sim's horizontal FOV means.
    pub fn azimuth_deg(self) -> f64 {
        self.x.atan2(self.z).to_degrees()
    }

    /// Vertical angle from straight ahead, in degrees. Positive is up.
    pub fn elevation_deg(self) -> f64 {
        self.y
            .atan2((self.x * self.x + self.z * self.z).sqrt())
            .to_degrees()
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn azimuth_is_signed_and_zero_dead_ahead() {
        assert!((Vec3::new(0.0, 0.0, 700.0).azimuth_deg()).abs() < 1e-12);
        assert!((Vec3::new(700.0, 0.0, 700.0).azimuth_deg() - 45.0).abs() < 1e-9);
        assert!((Vec3::new(-700.0, 0.0, 700.0).azimuth_deg() + 45.0).abs() < 1e-9);
        // Behind the eye, which is a state the validator warns about rather
        // than a state that may produce nonsense.
        assert!((Vec3::new(1.0, 0.0, -1.0).azimuth_deg() - 135.0).abs() < 1e-9);
    }

    #[test]
    fn elevation_ignores_lateral_position() {
        // A point up and to the right has the same elevation as one directly
        // up at the same distance in the ground plane.
        let a = Vec3::new(0.0, 100.0, 700.0).elevation_deg();
        let b = Vec3::new(0.0, 100.0, 700.0).yaw(0.5).elevation_deg();
        assert!((a - b).abs() < 1e-9);
    }

    #[test]
    fn yaw_turns_forward_toward_the_right() {
        let v = Vec3::new(0.0, 0.0, 1.0).yaw(std::f64::consts::FRAC_PI_2);
        assert!((v.x - 1.0).abs() < 1e-12 && v.z.abs() < 1e-12, "{v:?}");
    }

    #[test]
    fn rotations_preserve_length() {
        let v = Vec3::new(3.0, -4.0, 12.0);
        for r in [0.3, 1.1, -2.7] {
            for rotated in [v.yaw(r), v.pitch(r), v.roll(r)] {
                assert!((rotated.length() - 13.0).abs() < 1e-9);
            }
        }
    }
}
