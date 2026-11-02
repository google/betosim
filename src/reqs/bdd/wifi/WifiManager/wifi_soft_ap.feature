# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:soft_ap
Feature: WiFi Soft AP (Hotspot)

  @nyt
  Scenario: Enable Local Only Hotspot
    Given WiFi is enabled on the device
    When the device requests to start a Local Only Hotspot
    Then the system should enable the hotspot
    And return the generated SSID and password

  @nyt
  Scenario: Enable Full Tethering Hotspot
    Given the device has mobile data connectivity
    When the device requests to start WiFi Tethering
    Then the system should display a confirmation dialog or require system permissions
    And upon consent, enable the hotspot and share internet
