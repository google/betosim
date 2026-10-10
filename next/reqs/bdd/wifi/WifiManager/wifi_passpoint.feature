# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:passpoint
Feature: WiFi Passpoint (Hotspot 2.0)
  Reference: https://developer.android.com/reference/android/net/wifi/WifiManager#addOrUpdatePasspointConfiguration(android.net.wifi.PasspointConfiguration)

  @nyi
  Scenario: Automatic connection to Passpoint AP
    Given a device with a valid Passpoint profile installed
    And a Passpoint-enabled AP is in range and matching the profile
    When the device scans for networks
    Then the system should automatically authenticate and connect to the Passpoint AP
