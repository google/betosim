# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:state
Feature: WiFi State Management

  @nyt
  Scenario: Toggle WiFi State
    Given the device is connected to a WiFi network
    When the device disables WiFi
    Then the device should disconnect from the network
    And the WiFi state should be disabled
    When the device enables WiFi
    Then the device should reconnect to the saved network
