# Copyright 2026 The Android Open Source Project

Feature: Multi-step Coordination
  Scenario: Ping-Pong
    Given @adb has 1 attached device
    When @host starts a TCP echo server on "port"
    And @android:1 sends 2KB TCP to {port}
    And @android:1 sends 512B UDP to {port}
    Then @host receives all coordinated data
