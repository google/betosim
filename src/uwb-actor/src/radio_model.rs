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

use std::collections::BTreeMap;

use rand::prelude::*;

use crate::ranging::{CM_PER_METER, MAX_ELEVATION_DEGREES, MIN_ELEVATION_DEGREES};

const SIGMA_0_CM: f32 = 2.0;
const REF_DISTANCE_M: f32 = 5.0;
const AZIMUTH_SIGMA_DEG: f32 = 1.0;
const ELEVATION_SIGMA_DEG: f32 = 1.0;

fn round_to_u16(val: f32) -> u16 {
    val.clamp(0.0, f32::from(u16::MAX)).round() as u16
}

/// Sampled UWB ranging dataset with linear interpolation and parametric fallback.
///
/// Stores dataset (true distance, estimated distance) pairs grouped by integer centimeter
/// distances. When sampling a distance, exact matches sample uniformly from dataset measurements;
/// intermediate distances linearly interpolate between adjacent buckets; distances outside the
/// dataset fall back to distance-scaled zero-mean normal noise.
pub(crate) struct RangingDataSet {
    data: BTreeMap<u16, Vec<f32>>,
}

impl Default for RangingDataSet {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RangingDataSet {
    /// Creates a new `RangingDataSet` from the given dataset or defaults to `sample_ranging_data.csv`.
    fn new(ranging_data: Option<Vec<(f32, f32)>>) -> Self {
        let raw_data = ranging_data.unwrap_or_else(|| include!("sample_ranging_data.csv"));
        let mut data: BTreeMap<u16, Vec<f32>> = BTreeMap::new();
        for (true_m, est_m) in raw_data {
            let true_cm = round_to_u16(true_m * CM_PER_METER);
            let est_cm = est_m * CM_PER_METER;
            data.entry(true_cm).or_default().push(est_cm);
        }
        Self { data }
    }

    /// Samples an estimated distance for the given true distance in centimeters.
    fn sample_distance(&self, rng: &mut impl Rng, range_cm: f32) -> f32 {
        let distance_u16 = round_to_u16(range_cm);

        if let Some(sample) = self.data.get(&distance_u16).and_then(|v| v.choose(rng).copied()) {
            return sample;
        }

        let lower = self.data.range(..=distance_u16).next_back();
        let upper = self.data.range(distance_u16..).next();
        if let (Some((&lower_k, lower_vals)), Some((&upper_k, upper_vals))) = (lower, upper)
            && lower_k < upper_k
            && let (Some(&y1), Some(&y2)) = (lower_vals.choose(rng), upper_vals.choose(rng))
        {
            let x1 = lower_k as f32;
            let x2 = upper_k as f32;
            return y1 + (range_cm - x1) * ((y2 - y1) / (x2 - x1));
        }

        let d_m = range_cm / CM_PER_METER;
        let sigma_dist = SIGMA_0_CM * (d_m / REF_DISTANCE_M).max(1.0);
        range_cm + sample_normal(rng, sigma_dist)
    }

    /// Adds realistic measurement noise to UWB range and angle measurements.
    ///
    /// Range measurements are sampled from a de-biased 0.5m to 6.5m hardware dataset,
    /// with linear interpolation between buckets and a parametric bell curve fallback
    /// outside [0.5m, 6.5m]. The fallback uses a base standard deviation of `SIGMA_0_CM`
    /// (2 cm) up to `REF_DISTANCE_M` (5 m), scaling linearly with distance beyond 5 m,
    /// with all noise bounded within +/-3 sigma of the true value. Azimuth and elevation
    /// receive independent jitter (sigma = 1 deg).
    ///
    /// # Examples (back-of-the-napkin math)
    /// - Dataset range (d = 1m in [0.5m, 6.5m]):
    ///   - Sampled from the 1.0m dataset bucket -> range in [99, 103] cm.
    ///   - sigma_angle = 1 deg -> max jitter is +/-3 deg -> angle within +/-3 deg of true.
    /// - Fallback range (d = 20m > 6.5m):
    ///   - sigma_dist = 2 cm * (20m / 5m) = 8 cm -> max jitter is +/-3 sigma = +/-24 cm -> range in [1976, 2024] cm.
    ///   - sigma_angle = 1 deg -> max jitter is +/-3 deg -> angle within +/-3 deg of true.
    pub(crate) fn sample_ranging(
        &self,
        rng: &mut impl Rng,
        range_cm: f32,
        azimuth_deg: i16,
        elevation_deg: i8,
    ) -> (u16, i16, i8) {
        let sampled_range = round_to_u16(self.sample_distance(rng, range_cm));
        let az_noise = sample_normal(rng, AZIMUTH_SIGMA_DEG);
        let el_noise = sample_normal(rng, ELEVATION_SIGMA_DEG);

        let sampled_azimuth = wrap_azimuth(azimuth_deg as f32 + az_noise);
        let sampled_elevation = (elevation_deg as f32 + el_noise)
            .clamp(MIN_ELEVATION_DEGREES, MAX_ELEVATION_DEGREES)
            .round() as i8;

        (sampled_range, sampled_azimuth, sampled_elevation)
    }
}

/// Samples zero-mean bell curve noise with standard deviation `std_dev`.
///
/// Sums 3 uniform random numbers in [0, 1). Adding them forms a bell curve with
/// average 1.5 and standard deviation 0.5. Subtracting 1.5 and multiplying by 2
/// centers the noise at 0 with standard deviation 1.0, naturally bounded to [-3, 3] (+/-3 sigma).
fn sample_normal(rng: &mut impl Rng, std_dev: f32) -> f32 {
    let u1: f32 = rng.random();
    let u2: f32 = rng.random();
    let u3: f32 = rng.random();
    (u1 + u2 + u3 - 1.5) * 2.0 * std_dev
}

/// Wraps an azimuth angle in degrees into `[-180, 180)`, within the range allowed by UCI.
fn wrap_azimuth(deg: f32) -> i16 {
    let a = deg.round() as i32;
    let wrapped = (a + 180).rem_euclid(360) - 180;
    wrapped as i16
}

#[cfg(test)]
mod tests {
    use rand::{SeedableRng, rngs::StdRng};

    use super::*;

    #[test]
    fn test_dataset_lookup_and_interpolation() {
        let mut rng = StdRng::seed_from_u64(12345);
        let model = RangingDataSet::new(Some(vec![(0.5, 1.0), (0.5, 1.2), (1.0, 2.0), (1.0, 2.2)]));

        for _ in 0..100 {
            let d_50 = model.sample_distance(&mut rng, 50.0).round();
            assert!([100.0, 120.0].contains(&d_50));

            let d_100 = model.sample_distance(&mut rng, 100.0).round();
            assert!([200.0, 220.0].contains(&d_100));

            let d_75 = model.sample_distance(&mut rng, 75.0).round();
            assert!([150.0, 160.0, 170.0].contains(&d_75));

            let d_low = model.sample_distance(&mut rng, 25.0);
            assert!((19.0..=31.0).contains(&d_low));

            let d_high = model.sample_distance(&mut rng, 500.0);
            assert!((494.0..=506.0).contains(&d_high));
        }
    }

    #[test]
    fn test_default_ranging_and_aoa_bounds() {
        let mut rng = StdRng::seed_from_u64(42);
        let model = RangingDataSet::default();

        for _ in 0..100 {
            let (r, az, el) = model.sample_ranging(&mut rng, 100.0, 0, 0);
            assert!((99..=103).contains(&r));
            assert!((-3..=3).contains(&az));
            assert!((-3..=3).contains(&el));

            let (r_interp, _, _) = model.sample_ranging(&mut rng, 275.0, 0, 0);
            assert!((269..=281).contains(&r_interp));

            let (r_close_out, _, _) = model.sample_ranging(&mut rng, 30.0, 0, 0);
            assert!((24..=36).contains(&r_close_out));

            let (r_far, az_far, el_far) = model.sample_ranging(&mut rng, 2000.0, 45, -10);
            assert!((1976..=2024).contains(&r_far));
            assert!((42..=48).contains(&az_far));
            assert!((-13..=-7).contains(&el_far));

            let (r_out, _, _) = model.sample_ranging(&mut rng, 2500.0, 0, 0);
            assert!((2470..=2530).contains(&r_out));

            let (_, az_pos, el_pos) = model.sample_ranging(&mut rng, 100.0, 180, 90);
            assert!((-180..180).contains(&az_pos));
            assert!((-90..=90).contains(&el_pos));

            let (_, az_neg, el_neg) = model.sample_ranging(&mut rng, 100.0, -180, -90);
            assert!((-180..180).contains(&az_neg));
            assert!((-90..=90).contains(&el_neg));
        }
    }
}
