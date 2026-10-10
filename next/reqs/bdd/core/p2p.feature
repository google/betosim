# Copyright 2026 The Android Open Source Project

Feature: P2P Echo

Scenario: P2P TCP Echo
    Given @adb has 2 attached devices
    When @android:1 starts a TCP echo server on "target"
    And @android:2 sends 12 bytes of TCP data to {target}
    Then @android:1 receives 12 bytes of TCP data
