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

use std::sync::Arc;

use common::util::netsim_logger::{ErrorStatsLayer, ErrorStatsTable};
use tracing_subscriber::layer::SubscriberExt;

pub(crate) fn setup_error_stats_test()
-> (Arc<ErrorStatsTable>, impl tracing::Subscriber + Send + Sync, tracing::Dispatch) {
    let table = Arc::new(ErrorStatsTable::new());
    let subscriber =
        tracing_subscriber::registry().with(ErrorStatsLayer::with_table(Arc::clone(&table)));
    let multi_dispatch_guard = tracing::Dispatch::new(tracing_subscriber::registry());
    (table, subscriber, multi_dispatch_guard)
}
