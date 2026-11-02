# Copyright 2026 The Android Open Source Project

@epic:core @feature:coexistence
Feature: Multi-Radio Coexistence

  @nyi
  Scenario: WiFi high bandwidth affects Bluetooth performance
    Given a device with both WiFi and Bluetooth enabled
    And a Bluetooth audio stream is active and playing smoothly
    When a high-bandwidth WiFi file transfer begins on the same device
    Then the Bluetooth audio stream should experience packet loss or degradation
