# Copyright 2026 The Android Open Source Project

Feature: Multi-AVD Echo
  Scenario: Chained Echo
    Given @adb has 2 attached devices
    When @host starts a TCP echo server on "port"
    And @android:1 sends 1KB TCP to {port}
    And @android:2 sends 1KB TCP to {port}
    Then @host receives 2KB TCP data total
