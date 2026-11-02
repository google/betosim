# Copyright 2026 The Android Open Source Project

Feature: Bluetooth Advertisements

Scenario: Advertise and Scan
    Given @adb has 2 attached devices
    When @android:1 advertises with name "Netsim" and TxPower "HIGH"
    And @android:2 starts scanning
    Then @android:2 sees advertisement "Netsim" with RSSI "HIGH_POWER"
