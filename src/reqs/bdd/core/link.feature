# Copyright 2026 The Android Open Source Project

@ignore
Feature: Netsim Link Management

  Scenario: Link Lifecycle
    Given @adb has 2 attached devices
    When @netsim links @android:1 to @android:2 by bluetooth RSSI -70 as "link"
    Then @netsim should list a link with RSSI -70
    When @netsim patches link {link} with RSSI -85
    Then @netsim should list a link with RSSI -85
    When @netsim deletes link {link}

  Scenario: Scan Result with Configured RSSI
    Given @adb has 2 attached devices
    When @netsim links @android:1 to @android:2 by bluetooth RSSI -60 as "link"
    And @android:1 advertises with name "Netsim" and TxPower "HIGH"
    And @android:2 starts scanning
    Then @android:2 sees advertisement "Netsim" with RSSI "-60"
    When @netsim deletes link {link}
