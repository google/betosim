// Copyright 2026 The Android Open Source Project
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Ranging utilities for UWB spatial computations.

use glam::{EulerRot, Quat, Vec3};

use crate::UwbError;

pub(crate) const CM_PER_METER: f32 = 100.0;
const MIN_AZIMUTH_DEGREES: f32 = -180.0;
const MAX_AZIMUTH_DEGREES: f32 = 180.0;
pub(crate) const MIN_ELEVATION_DEGREES: f32 = -90.0;
pub(crate) const MAX_ELEVATION_DEGREES: f32 = 90.0;

/// helper function for performing division with zero division check
fn checked_div(num: f32, den: f32) -> Option<f32> {
    (den != 0.).then_some(num / den)
}

/// helper function for calculating azimuth angle from a given 3D delta vector.
fn azimuth(delta: Vec3) -> f32 {
    checked_div(delta.x, delta.z).map_or(
        match delta.x == 0. {
            true => 0.,
            false => delta.x.signum() * std::f32::consts::FRAC_PI_2,
        },
        f32::atan,
    ) + if delta.z >= 0. { 0. } else { delta.x.signum() * std::f32::consts::PI }
}

/// helper function for calculating elevation angle from a given 3D delta
/// vector.
fn elevation(delta: Vec3) -> f32 {
    checked_div(delta.y, f32::sqrt(delta.x.powi(2) + delta.z.powi(2))).map_or(
        match delta.y == 0. {
            true => 0.,
            false => delta.y.signum() * std::f32::consts::FRAC_PI_2,
        },
        f32::atan,
    )
}

/// Internal Pose struct for mathematical representation.
pub(crate) struct Pose {
    position: Vec3,
    orientation: Quat,
}

impl From<&netsim_model::Pose> for Pose {
    fn from(pose: &netsim_model::Pose) -> Self {
        Pose {
            // Converts x, y, z from meters to centimeters
            position: Vec3::new(
                pose.position.x * CM_PER_METER,
                pose.position.y * CM_PER_METER,
                pose.position.z * CM_PER_METER,
            ),
            // Converts roll, pitch, yaw from degrees to radians
            orientation: Quat::from_euler(
                EulerRot::ZXY,
                pose.orientation.roll.to_radians(),
                pose.orientation.pitch.to_radians(),
                pose.orientation.yaw.to_radians(),
            ),
        }
    }
}

impl Pose {
    /// Computes range, azimuth, and elevation between this pose and a target pose.
    ///
    /// The ranging model is adapted from https://github.com/google/pica.
    pub(crate) fn compute_ranging(&self, target: &Pose) -> Result<(f32, i16, i8), UwbError> {
        let delta = target.position - self.position;
        let distance = delta.length().clamp(0.0, u16::MAX as f32);
        let direction = self.orientation.mul_vec3(delta);
        let azimuth = azimuth(direction).to_degrees().round();
        let elevation = elevation(direction).to_degrees().round();

        if !(MIN_AZIMUTH_DEGREES..=MAX_AZIMUTH_DEGREES).contains(&azimuth) {
            return Err(UwbError::InvalidAzimuth(azimuth));
        }
        if !(MIN_ELEVATION_DEGREES..=MAX_ELEVATION_DEGREES).contains(&elevation) {
            return Err(UwbError::InvalidElevation(elevation));
        }
        Ok((distance, azimuth as i16, elevation as i8))
    }
}

#[cfg(test)]
mod tests {
    use netsim_model::{Orientation, Pose, Position};

    fn create_pose(x: f32, y: f32, z: f32, yaw: f32, pitch: f32, roll: f32) -> super::Pose {
        super::Pose::from(&Pose {
            position: Position { x, y, z },
            orientation: Orientation { yaw, pitch, roll },
        })
    }

    #[test]
    fn range() {
        let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        {
            let b_pose = create_pose(10.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            let (range, _, _) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(range, 1000.);
        }
        {
            let b_pose = create_pose(-10.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            let (range, _, _) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(range, 1000.);
        }
        {
            let b_pose = create_pose(10.0, 10.0, 0.0, 0.0, 0.0, 0.0);
            let (range, _, _) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(range, f32::sqrt(2000000.));
        }
        {
            let b_pose = create_pose(-10.0, -10.0, -10.0, 0.0, 0.0, 0.0);
            let (range, _, _) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(range, f32::sqrt(3000000.));
        }
    }

    #[test]
    fn range_zero_distance() {
        let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let b_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let (range, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
        assert_eq!(range, 0.0);
        assert_eq!(azimuth, 0);
        assert_eq!(elevation, 0);
    }

    #[test]
    fn azimuth_without_rotation() {
        let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        {
            let b_pose = create_pose(10.0, 0.0, 10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 45);
            assert_eq!(elevation, 0);
        }
        {
            let b_pose = create_pose(-10.0, 0.0, 10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, -45);
            assert_eq!(elevation, 0);
        }
        {
            let b_pose = create_pose(10.0, 0.0, -10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 135);
            assert_eq!(elevation, 0);
        }
        {
            let b_pose = create_pose(-10.0, 0.0, -10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, -135);
            assert_eq!(elevation, 0);
        }
    }

    #[test]
    fn elevation_without_rotation() {
        let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        {
            let b_pose = create_pose(0.0, 10.0, 10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 0);
            assert_eq!(elevation, 45);
        }
        {
            let b_pose = create_pose(0.0, -10.0, 10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 0);
            assert_eq!(elevation, -45);
        }
        {
            let b_pose = create_pose(0.0, 10.0, -10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert!(azimuth == 180 || azimuth == -180);
            assert_eq!(elevation, 45);
        }
        {
            let b_pose = create_pose(0.0, -10.0, -10.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert!(azimuth == 180 || azimuth == -180);
            assert_eq!(elevation, -45);
        }
    }

    #[test]
    fn rotation_only() {
        let b_pose = create_pose(0.0, 0.0, 10.0, 0.0, 0.0, 0.0);
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 0);
            assert_eq!(elevation, 0);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 45.0, 0.0, 0.0); // <=> azimuth = -45deg
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 45);
            assert_eq!(elevation, 0);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 45.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 0);
            assert_eq!(elevation, -45);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 45.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 0);
            assert_eq!(elevation, 0);
        }
    }

    #[test]
    fn rotation_only_complex_position() {
        let b_pose = create_pose(10.0, 10.0, 10.0, 0.0, 0.0, 0.0);
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 45);
            assert_eq!(elevation, 35);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 90.0, 0.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 135);
            assert_eq!(elevation, 35);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 90.0, 0.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 45);
            assert_eq!(elevation, -35);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, 0.0, 0.0, 90.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, -45);
            assert_eq!(elevation, 35);
        }
        {
            let a_pose = create_pose(0.0, 0.0, 0.0, -45.0, 35.0, 42.0);
            let (_, azimuth, elevation) = a_pose.compute_ranging(&b_pose).unwrap();
            assert_eq!(azimuth, 0);
            assert_eq!(elevation, 0);
        }
    }
}
