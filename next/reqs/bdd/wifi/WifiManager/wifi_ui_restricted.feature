# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:ui_restricted
Feature: WiFi UI Restricted Operations

  @nyt
  Scenario: App requests to enable WiFi via Settings Panel
    Given WiFi is disabled
    And a regular app requests to enable WiFi
    Then the system should display the Settings Panel for WiFi
    # Note: This requires UI interaction to simulate user clicking "Turn on"

  @nyt
  Scenario: User consent required for network connection
    Given a regular app requests connection to SSID "TargetNet" via NetworkSpecifier
    Then the system should display a consent dialog asking to connect to "TargetNet"
    # Note: This requires UI interaction to simulate user clicking "Allow"
