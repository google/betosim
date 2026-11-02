# Copyright 2026 The Android Open Source Project

Feature: Netsim Observables Verification

  Scenario: Verify connected device count
    Given @adb has 1 attached device
    Then @netsim observes "connected-devices" should be ">=1"

  Scenario: Verify device count and valid version
    Given @adb has 1 attached device
    Then @netsim observes:
      | connected-devices | >=1     |
      | netsim-version    | *      |
