# Copyright 2026 The Android Open Source Project

Feature: Gateway Performance
  Scenario: Benchmark
    Given @adb has 1 attached device
    And @netsim is running
    When @host starts a TCP echo server on "port"
    Then @android:1 measures performance with 10 samples of 1MB TCP to {port}
    And @host receives all coordinated data
