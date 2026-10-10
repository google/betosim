# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:aware
Feature: WiFi Aware (NAN)
  Reference: https://developer.android.com/reference/android/net/wifi/aware/WifiAwareManager

  @nyi
  Scenario: Discover published service with ranging
    Given device A supports WiFi Aware with ranging
    And device B is publishing a service "PrintService" over WiFi Aware
    When device A subscribes to "PrintService"
    Then device A should discover device B
    And device A should receive a distance estimate to device B

  @nyt
  Scenario: Basic Publish and Subscribe
    Given device A supports WiFi Aware
    And device B is publishing a service "ChatService" over WiFi Aware
    When device A subscribes to "ChatService"
    Then device A should discover device B

  @nyt
  Scenario: Message Exchange between Peers
    Given device A has discovered device B's service over WiFi Aware
    When device A sends a message "Hello" to device B
    Then device B should receive the message "Hello"

  @nyi
  Scenario: Establish Data Path
    Given device A has discovered device B's service over WiFi Aware
    When they request to establish a NAN data path
    Then a network connection should be established between them
