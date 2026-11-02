# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:suggestions
Feature: WiFi Network Suggestions
  Reference: https://developer.android.com/reference/android/net/wifi/WifiNetworkSuggestion

  @nyt
  Scenario: Connect to suggested network
    Given an app has registered a network suggestion for SSID "SuggestedNet"
    And "SuggestedNet" AP is available and in range
    When the system evaluates available networks
    Then the system should automatically connect to "SuggestedNet"
