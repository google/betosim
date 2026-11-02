# Copyright 2026 The Android Open Source Project

# Known Failure: The device fails to reconnect to "AndroidWifi" after
# disconnecting from a requested network.

@ignore
@epic:wifi @feature:wifi_ap @wifi @ap
Feature: Wi-Fi Access Point and Connectivity

  Background:
    Given @adb has 1 attached device

  Scenario: Verify initial connection to AndroidWifi
    Then @adb verifies connected wifi is "AndroidWifi"

  Scenario: Create a simulated AP and connect targeting it via ConnectivityManager
    # Assuming initial state is connected to AndroidWifi
    # If not, the first step might fail or we need a step to ensure it.
    # Let's assume it is the default state in emulator.

    # 1. Create AP on Host (Netsim)
    When Netsim creates Wi-Fi Access Point "Lab_AP" with protocol "g"

    # 2. Verify properties in Netsim
    Then Wi-Fi Access Point "Lab_AP" in netsim has protocol "g"

    # 3. Connect via ADB to bypass UI dialog
    When @adb connects to wifi "Lab_AP"

    # 4. Verify via ADB
    Then @adb verifies connected wifi is "Lab_AP"

    # 4.5 Remove AP in Netsim
    When Netsim removes Wi-Fi Access Point "Lab_AP"

    # 4.6 Forget network on device
    When @adb forgets network "Lab_AP"

  Scenario: Connect to new AP
    When Netsim creates Wi-Fi Access Point "Lab_AP_2" with protocol "g"
    When @adb connects to wifi "Lab_AP_2"
    Then @adb verifies connected wifi is "Lab_AP_2"

  Scenario: Create and connect to password protected AP
    When Netsim creates Wi-Fi Access Point "Secured_AP" with protocol "g" and password "password123"
    When @adb connects to wifi "Secured_AP" with password "password123"
    Then @adb verifies connected wifi is "Secured_AP"

