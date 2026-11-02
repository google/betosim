# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:scanning
Feature: WiFi Scanning

  @nyt
  Scenario: Scan for available networks
    Given multiple APs are available in range
    When the device triggers a WiFi scan
    Then the system should return a list of scan results
    And the results should include SSID and RSSI for all in-range APs
