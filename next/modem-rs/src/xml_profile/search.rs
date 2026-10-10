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

use super::schema::*;
use crate::constants::UiccFileId;

fn find_in_df<T, F>(df: &XmlDedicatedFile, file_id: impl Into<u16>, extractor: &F) -> Option<T>
where
    F: Fn(&XmlElementaryFileMember) -> Option<T>,
{
    let file_id = file_id.into();
    df.members.iter().find_map(|member| match member {
        XmlDedicatedFileMember::Elementary(ef) => {
            if ef.id == file_id {
                ef.members.iter().find_map(extractor)
            } else {
                None
            }
        }
        XmlDedicatedFileMember::Dedicated(sub_df) => find_in_df(sub_df, file_id, extractor),
        XmlDedicatedFileMember::ApplicationDedicated(adf) => find_in_adf(adf, file_id, extractor),
    })
}

fn find_in_adf<T, F>(
    adf: &XmlApplicationDedicatedFile,
    file_id: impl Into<u16>,
    extractor: &F,
) -> Option<T>
where
    F: Fn(&XmlElementaryFileMember) -> Option<T>,
{
    let file_id = file_id.into();
    adf.members.iter().find_map(|member| match member {
        XmlApplicationDedicatedFileMember::ElementaryFile(ef) => {
            if ef.id == file_id {
                ef.members.iter().find_map(extractor)
            } else {
                None
            }
        }
        XmlApplicationDedicatedFileMember::DedicatedFile(df) => find_in_df(df, file_id, extractor),
        _ => None,
    })
}

pub fn find_ccid(mf: &XmlDedicatedFile) -> Option<String> {
    find_in_df(mf, UiccFileId::Iccid, &|member| match member {
        XmlElementaryFileMember::Ccid(ccid) => Some(ccid.clone()),
        _ => None,
    })
}

pub fn find_imsi(
    adfs: &[XmlApplicationDedicatedFile],
    mf: Option<&XmlDedicatedFile>,
) -> Option<String> {
    let extractor = |member: &XmlElementaryFileMember| match member {
        XmlElementaryFileMember::Cimi(cimi) => Some(cimi.clone()),
        _ => None,
    };
    for adf in adfs {
        if let Some(imsi) = find_in_adf(adf, UiccFileId::Imsi, &extractor) {
            return Some(imsi);
        }
    }
    mf.and_then(|df| find_in_df(df, UiccFileId::Imsi, &extractor))
}
