# Copyright 2026 The Android Open Source Project

Feature: Echo
  Scenario: Simple Echo
    Given @adb has 1 attached device
    When @host starts a TCP echo server on "port"
    And @android:1 sends 1KB TCP to {port}
    Then @host receives 1KB TCP data
