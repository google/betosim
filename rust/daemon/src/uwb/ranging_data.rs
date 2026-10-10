// Copyright 2024 The Android Open Source Project
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

#![allow(deprecated)]

use rand::{prelude::*, rng, rngs::ThreadRng};

use std::collections::BTreeMap;

type TrueDistance = f32; // meters
type EstimatedDistance = f32; // meters

/// The data is organized as a `BTreeMap` for efficient lookup and interpolation.
pub struct RangingDataSet {
    /// Stores ranging data in the form of (true distance, [estimated distances]) pairs.
    /// The map keys are true distances in u16 centimeters.
    data: BTreeMap<u16, Vec<EstimatedDistance>>,
}

impl RangingDataSet {
    /// Creates a new `RangingDataSet` instance by loading ranging data.
    /// Data is in a format where each entry is a tuple of
    /// (true distance, estimated distance), typically representing samples from a
    /// ranging sensor.
    pub fn new(ranging_data: Option<Vec<(TrueDistance, EstimatedDistance)>>) -> Self {
        // Use sample_ranging_data.csv if ranging_data is not provided.
        #[allow(clippy::excessive_precision)]
        let mut sample_ranging_data: Vec<(TrueDistance, EstimatedDistance)> =
            ranging_data.unwrap_or(include!("sample_ranging_data.csv"));
        // Convert to centimeters as Pica uses centimeters for ranging units
        sample_ranging_data = sample_ranging_data
            .into_iter()
            .map(|(true_dist, est_dist)| (true_dist * 100.0, est_dist * 100.0))
            .collect::<Vec<(TrueDistance, EstimatedDistance)>>();

        // Process the sample_raning_data into BTreeMap
        let mut data: BTreeMap<u16, Vec<EstimatedDistance>> = BTreeMap::new();
        for (true_distance, estimated_distance) in sample_ranging_data {
            // Convert true_distance into u16 centimeters
            data.entry((true_distance * 100.0).round() as u16)
                .or_default()
                .push(estimated_distance);
        }
        RangingDataSet { data }
    }

    /// Samples an estimated distance for the given true distance.
    ///
    /// # Arguments
    ///
    /// * `distance` - The true distance for which an estimated distance is required.
    /// * `option_rng` - An optional random number generator
    ///   (if not provided, a default one will be used)
    ///
    /// # Returns
    ///
    /// An estimated distance sampled from the dataset.
    /// If the exact true distance is found in the dataset,
    ///     a random estimated distance is chosen from its associated values.
    /// If the true distance falls between known values,
    ///     linear interpolation is used to estimate a distance.
    /// If the true distance is outside the range of known values,
    ///     the distance itself is returned as the estimated distance.
    pub fn sample(
        &self,
        distance: TrueDistance,
        option_rng: Option<ThreadRng>,
    ) -> EstimatedDistance {
        // Generate a new ThreadRng if not provided
        let mut rng = option_rng.unwrap_or(rng());
        // Convert TrueDistance into u16 centimeters.
        let distance_u16 = (distance * 100.0).round() as u16;

        // Random sampling if distance is an existing data key
        if let Some(vec_estimated_distance) = self.data.get(&distance_u16) {
            return *vec_estimated_distance.choose(&mut rng).unwrap();
        }

        // Linear Interpolation if provided TrueDistance lies in between data keys
        let lower = self.data.range(..=&distance_u16).next_back();
        let upper = self.data.range(&distance_u16..).next();
        match (lower, upper) {
            (Some((lower_key, lower_vals)), Some((upper_key, upper_vals))) => {
                let x1 = *lower_key as f32 / 100.0;
                let y1 = *lower_vals.choose(&mut rng).unwrap();
                let x2 = *upper_key as f32 / 100.0;
                let y2 = *upper_vals.choose(&mut rng).unwrap();
                y1 + (distance - x1) * (y2 - y1) / (x2 - x1)
            }
            _ => distance,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ranging_data_set() -> RangingDataSet {
        let ranging_data = vec![(0.0, 0.2), (1.0, 0.9), (2.0, 1.9), (2.0, 2.1)];
        RangingDataSet::new(Some(ranging_data))
    }

    #[test]
    fn test_sample_ranging_data_set() {
        let ranging_data_set = sample_ranging_data_set();
        // Linear Interpolation
        assert_eq!(ranging_data_set.sample(50., None), 55.);
        // Exact distance found in dataset
        assert!([190., 210.].contains(&ranging_data_set.sample(200., None).round()));
        // Out of Range
        assert_eq!(ranging_data_set.sample(300., None), 300.);
    }
}
