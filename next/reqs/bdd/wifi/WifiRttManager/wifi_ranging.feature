# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:ranging
Feature: WiFi Ranging
  Reference: https://developer.android.com/reference/android/net/wifi/rtt/WifiRttManager

  @nyi
  Scenario: WiFi RTT Ranging
    Given two devices supporting WiFi RTT
    When one device requests ranging to the other using RTT
    Then the system should return the distance based on RTT

  @nyt
  Scenario: WiFi RSSI Ranging
    Given a device connected to an AP
    When the device moves and requests signal strength
    Then the system should return the updated RSSI

  @nyi
  Scenario: WiFi RTT Ranging Failure - Out of Range
    Given two devices supporting WiFi RTT
    And the devices are beyond the maximum simulated RTT range
    When one device requests ranging to the other using RTT
    Then the system should return a ranging result with failure status

  @nyi
  Scenario: WiFi RTT Ranging to Multiple APs
    Given a device supporting WiFi RTT
    And multiple APs supporting RTT in range
    When the device requests ranging to the list of APs
    Then the system should return a list of ranging results for all APs

  @nyi
  Scenario: WiFi RTT Ranging with LCI Request
    Given a device supporting WiFi RTT
    And an AP with LCI (Location Configuration Information) configured
    When the device requests ranging to the AP with LCI enabled
    Then the system should return the distance
    And the result should include the LCI data

  @nyi
  Scenario: WiFi RTT Ranging to WiFi Aware Peer
    Given two devices supporting WiFi Aware and RTT
    And they have discovered each other via WiFi Aware
    When one device requests ranging to the other's PeerHandle
    Then the system should return the distance based on RTT
