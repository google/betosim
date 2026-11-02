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

use hex;
use modem_rs::{
    DedicatedFile, ElementaryFile, FileSystem, PhoneNumber, PinState, SimFile, SimIo, SimProfile,
    config::PinProfile, constants::UiccFileId, test_utils::MockModemHandler,
};
use netsim_model::{CellNetworkConfig, Quirks, RegistrationStatus};

use crate::{common::constants::*, world::World};

pub fn create_default_test_network_config() -> CellNetworkConfig {
    CellNetworkConfig {
        ip_address: TEST_IPV4_ADDR.into(),
        prefixlen: 24,
        gateway: TEST_GATEWAY_IPV4.into(),
        dns: TEST_DNS_IPV4.into(),
    }
}

fn set_registered_home(world: &mut World, id: u32) {
    if let Some(modem) = world.manager.get_modem_mut(id) {
        modem.set_voice_registration(RegistrationStatus::RegisteredHome);
        modem.set_data_registration(RegistrationStatus::RegisteredHome);
    }
}

pub fn given_modem_with_network_config(
    world: &mut World,
    name: &str,
    network_config: CellNetworkConfig,
) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(false);

    world
        .manager
        .new_modem(id, sink, None, None, Quirks::default(), vec![network_config])
        .expect("Failed to create new modem with network config");
    world.modems.insert(name.to_string(), (id, handler));
    set_registered_home(world, id);
}

pub fn given_data_modem(world: &mut World, name: &str) {
    given_data_modem_with_quirks(world, name, Quirks::default());
}

pub fn given_data_modem_with_quirks(world: &mut World, name: &str, quirks: Quirks) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(quirks.goldfish_ril_37_or_earlier);

    world
        .manager
        .new_modem(id, sink, None, None, quirks, vec![create_default_test_network_config()])
        .expect("Failed to create new modem with network config");
    world.modems.insert(name.to_string(), (id, handler));
    set_registered_home(world, id);
}

pub fn given_goldfish_37_data_modem(world: &mut World, name: &str) {
    given_data_modem_with_quirks(
        world,
        name,
        Quirks { goldfish_ril_37_or_earlier: true, ..Default::default() },
    );
}

/// Creates a modem with the given name, leaving registration NotRegistered by default (unlike given_modem_with_number).
///
/// # Panics
///
/// Panics if a modem with the same name already exists or if creation fails.
pub fn given_modem(world: &mut World, name: &str) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(false);

    world
        .manager
        .new_modem(id, sink, None, None, Quirks::default(), Vec::new())
        .expect("Failed to create new modem");
    world.modems.insert(name.to_string(), (id, handler));
}

/// Creates a modem with custom quirks.
pub fn given_modem_with_quirks(world: &mut World, name: &str, quirks: Quirks) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(quirks.goldfish_ril_37_or_earlier);

    world
        .manager
        .new_modem(id, sink, None, None, quirks, Vec::new())
        .expect("Failed to create new modem");
    world.modems.insert(name.to_string(), (id, handler));
}

/// Creates a Goldfish 37 (or earlier) modem with the given name.
pub fn given_goldfish_37_modem(world: &mut World, name: &str) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(true);

    let quirks = Quirks { goldfish_ril_37_or_earlier: true, ..Default::default() };
    world
        .manager
        .new_modem(id, sink, None, None, quirks, Vec::new())
        .expect("Failed to create new modem");
    world.modems.insert(name.to_string(), (id, handler));
}

/// Creates a modem with the given name and SIM type.
pub fn given_modem_with_sim_type(world: &mut World, name: &str, sim_type: i32) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(false);

    world
        .manager
        .new_modem(id, sink, Some(sim_type), None, Quirks::default(), Vec::new())
        .expect("Failed to create new modem");
    world.modems.insert(name.to_string(), (id, handler));
}

/// Creates a Cuttlefish modem with the given name.
pub fn given_cuttlefish_modem(world: &mut World, name: &str) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(false);

    let quirks = Quirks { is_cuttlefish: true, ..Default::default() };
    world
        .manager
        .new_modem(id, sink, None, None, quirks, Vec::new())
        .expect("Failed to create new modem");
    world.modems.insert(name.to_string(), (id, handler));
}

/// Creates a modem with the given name and phone number.
///
/// This first creates the modem, then sets its phone number via `ModemImpl`.
pub fn given_modem_with_number(world: &mut World, name: &str, number: &str) {
    given_modem(world, name);
    let (id, _) = world.get_modem(name);
    if let Some(modem) = world.manager.get_modem_mut(id) {
        let phone = PhoneNumber::new_for_test(number);
        modem.set_phone_number(phone);
    } else {
        panic!("Failed to retrieve modem '{name}' after creation");
    }
    set_registered_home(world, id);
}

/// Creates a modem with the default test SIM profile (legacy behavior).
///
/// This profile includes specific ICCID, IMSI ("123456789012345"), and a file
/// system with `2FE2`.
pub fn given_modem_with_sim_profile(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_legacy_test_profile());
}

/// Creates a modem with a custom XML SIM profile.
pub fn given_modem_with_xml_profile(world: &mut World, name: &str, xml: &str) {
    if world.modems.contains_key(name) {
        panic!("Modem with name '{name}' already exists");
    }

    let id = world.next_modem_id();
    let (handler, sink) = MockModemHandler::new(false);

    world
        .manager
        .new_modem(id, sink, None, Some(xml.to_string()), Quirks::default(), Vec::new())
        .expect("Failed to create new modem with XML profile");
    world.modems.insert(name.to_string(), (id, handler));
}

/// Helper function to create the legacy SIM profile used in tests.
pub fn create_legacy_test_profile() -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![SimFile::ElementaryFile(ElementaryFile {
                        file_id: UiccFileId::Iccid.into(),

                        record_len: None,
                        data: hex::decode(TEST_ICCID).unwrap(),
                    })],
                },
            },
        },
        ..Default::default()
    }
}

/// Helper function to create a SIM profile with PIN lock enabled but not
/// verified.
pub fn create_locked_sim_profile() -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        pin_profile: PinProfile {
            state: PinState::EnabledNotVerified,
            pin1: LOCKED_PIN.to_string(),
            puk1: TEST_PUK.to_string(),
            ..Default::default()
        },
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![SimFile::ElementaryFile(ElementaryFile {
                        file_id: UiccFileId::Iccid.into(),

                        record_len: None,
                        data: hex::decode(TEST_ICCID).unwrap(),
                    })],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a locked SIM profile (legacy test profile with PIN
/// EnabledNotVerified).
pub fn given_modem_with_locked_sim(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_locked_sim_profile());
}

/// Helper function to create a SIM profile with PIN permanently blocked.
pub fn create_perm_blocked_sim_profile() -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        pin_profile: PinProfile {
            state: PinState::PermBlocked,
            pin1: LOCKED_PIN.to_string(),
            puk1: TEST_PUK.to_string(),
            puk1_retries: Some(0),
            ..Default::default()
        },
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![SimFile::ElementaryFile(ElementaryFile {
                        file_id: UiccFileId::Iccid.into(),

                        record_len: None,
                        data: hex::decode(TEST_ICCID).unwrap(),
                    })],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a permanently blocked SIM profile.
pub fn given_modem_with_perm_blocked_sim(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_perm_blocked_sim_profile());
}

/// Helper function to create a SIM profile with EF_MSISDN in the filesystem.
pub fn create_profile_with_msisdn() -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::Iccid.into(),

                            record_len: None,
                            data: hex::decode(TEST_ICCID).unwrap(),
                        }),
                        SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.into(),
                            files: vec![SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::Msisdn.into(),

                                record_len: UiccFileId::Msisdn.default_record_len(),
                                data: vec![
                                    0xFF;
                                    UiccFileId::Msisdn
                                        .default_record_len()
                                        .expect("file id is record based")
                                ],
                            })],
                        }),
                    ],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a SIM profile that has EF_MSISDN in the filesystem.
pub fn given_modem_with_msisdn_in_fs(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_profile_with_msisdn());
}

/// Helper function to create a SIM profile with a 2-record EF_MSISDN in the
/// filesystem.
pub fn create_profile_with_multi_record_msisdn() -> SimProfile {
    let line1 = hex::decode("4C696E6531FFFFFFFFFFFFFFFFFF07915155111111F1FFFFFFFFFFFF").unwrap();
    let line2 = hex::decode("4C696E6532FFFFFFFFFFFFFFFFFF07915155222222F2FFFFFFFFFFFF").unwrap();
    let mut data = Vec::new();
    data.extend(line1);
    data.extend(line2);

    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::Iccid.into(),
                            record_len: None,
                            data: hex::decode(TEST_ICCID).unwrap(),
                        }),
                        SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.into(),
                            files: vec![SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::Msisdn.into(),
                                record_len: UiccFileId::Msisdn.default_record_len(),
                                data,
                            })],
                        }),
                    ],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a SIM profile that has a 2-record EF_MSISDN in the
/// filesystem.
pub fn given_modem_with_multi_record_msisdn_in_fs(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_profile_with_multi_record_msisdn());
}

/// Helper function to create a SIM profile with a custom record length
/// EF_MSISDN.
pub fn create_profile_with_custom_record_len_msisdn(record_len: usize) -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::Iccid.into(),
                            record_len: None,
                            data: hex::decode(TEST_ICCID).unwrap(),
                        }),
                        SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.into(),
                            files: vec![SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::Msisdn.into(),
                                record_len: Some(record_len),
                                data: vec![0xFF; record_len],
                            })],
                        }),
                    ],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a SIM profile that has a custom record length EF_MSISDN
/// in the filesystem.
pub fn given_modem_with_custom_record_len_msisdn_in_fs(
    world: &mut World,
    name: &str,
    record_len: usize,
) {
    world.given_modem_with_profile(name, create_profile_with_custom_record_len_msisdn(record_len));
}

/// Helper function to create a SIM profile with EF_FPLMN and EF_MBDN in the
/// filesystem.
pub fn create_profile_with_fplmn_and_mbdn() -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::Iccid.into(),

                            record_len: None,
                            data: hex::decode(TEST_ICCID).unwrap(),
                        }),
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::ForbiddenPlmn.into(),

                            record_len: None,
                            data: vec![0xFF; 12],
                        }),
                        SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.as_u16(),
                            files: vec![SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::MailboxDialingNumbers.into(),
                                record_len: UiccFileId::MailboxDialingNumbers.default_record_len(),
                                data: vec![
                                    0xFF;
                                    4 * UiccFileId::MailboxDialingNumbers
                                        .default_record_len()
                                        .expect("file id is record based")
                                ],
                            })],
                        }),
                    ],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a SIM profile that has EF_FPLMN and EF_MBDN in the
/// filesystem.
pub fn given_modem_with_fplmn_and_mbdn_in_fs(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_profile_with_fplmn_and_mbdn());
}

/// Helper function to create a SIM profile with FDN records.
pub fn create_fdn_sim_profile() -> SimProfile {
    let fdn_record1 =
        hex::decode("FFFFFFFFFFFFFFFFFFFFFFFFFFFF04812143F5FFFFFFFFFFFFFFFFFF").unwrap();
    let fdn_record2 =
        hex::decode("FFFFFFFFFFFFFFFFFFFFFFFFFFFF07916105550501F0FFFFFFFFFFFF").unwrap();
    let fdn_record3 =
        hex::decode("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF").unwrap();
    let mut fdn_data = Vec::new();
    fdn_data.extend(fdn_record1);
    fdn_data.extend(fdn_record2);
    fdn_data.extend(fdn_record3);

    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        pin_profile: PinProfile { pin2: "5678".to_string(), ..Default::default() },
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::Iccid.into(),
                            record_len: None,
                            data: hex::decode(TEST_ICCID).unwrap(),
                        }),
                        SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.into(),
                            files: vec![SimFile::ElementaryFile(ElementaryFile {
                                file_id: UiccFileId::FixedDialingNumbers.into(),
                                record_len: UiccFileId::FixedDialingNumbers.default_record_len(),
                                data: fdn_data,
                            })],
                        }),
                    ],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a SIM profile that has FDN records.
pub fn given_modem_with_fdn_sim_profile(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_fdn_sim_profile());
    let (id, _) = world.get_modem(name);
    set_registered_home(world, id);
}

/// Creates a 2G GSM SIM profile with MF (0x3F00) and DF_TELECOM (0x7F10)
/// without any Application Dedicated Files (ADFs).
pub fn create_2g_sim_profile() -> SimProfile {
    SimProfile {
        iccid: TEST_ICCID.to_string(),
        imsi: TEST_IMSI.to_string(),
        sim_io: SimIo {
            file_system: FileSystem {
                master_file: DedicatedFile {
                    file_id: UiccFileId::MasterFile.into(),
                    files: vec![
                        SimFile::ElementaryFile(ElementaryFile {
                            file_id: UiccFileId::Iccid.into(),
                            record_len: None,
                            data: hex::decode(TEST_ICCID).unwrap(),
                        }),
                        SimFile::DedicatedFile(DedicatedFile {
                            file_id: UiccFileId::Telecom.into(),
                            files: Vec::new(),
                        }),
                    ],
                },
            },
        },
        ..Default::default()
    }
}

/// Creates a modem with a 2G SIM profile (no ADFs).
pub fn given_modem_with_2g_sim_profile(world: &mut World, name: &str) {
    world.given_modem_with_profile(name, create_2g_sim_profile());
}
