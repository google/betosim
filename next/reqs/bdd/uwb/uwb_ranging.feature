# Copyright 2026 The Android Open Source Project

@uwb
Feature: Android UWB Ranging

  Scenario: UWB Ranging between two Android devices
    Given @adb has 2 attached devices
    And @netsim is running

    # Establish UWB sessions
    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with address B

    # Now valid ranging
    When @android:1 starts UWB ranging with peer {B}
    And @android:2 starts UWB ranging with peer {A}

    Then @android:2 UWB peer {A} is connected

    # Verify Measurements
    Then @android:2 UWB distance to {A} is 0.0m (+/- 0.05m)
    And @android:2 UWB azimuth to {A} is 0.0 degrees (+/- 5.0 degrees)
    And @android:2 UWB elevation to {A} is 0.0 degrees (+/- 5.0 degrees)

  Scenario: Stop UWB Ranging between two Android devices
    Given @adb has 2 attached devices
    And @netsim is running

    # Establish UWB sessions
    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with address B

    # Start and stop ranging
    When @android:1 starts UWB ranging with peer {B}
    And @android:2 starts UWB ranging with peer {A}
    Then @android:2 UWB peer {A} is connected

    When @android:1 Stops UWB Ranging
    And @android:2 Stops UWB Ranging
    Then @android:2 UWB peer {A} is disconnected

  Scenario: UWB Ranging with Distance Only (Config 3)
    Given @adb has 2 attached devices
    And @netsim is running

    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with address B

    # Start ranging with config 3 (Often pre-defined as Distance only)
    When @android:1 starts UWB ranging with config 3 and peer {B}
    And @android:2 starts UWB ranging with config 3 and peer {A}

    Then @android:2 UWB peer {A} is connected

    Then @android:2 UWB distance to {A} is 0.0m (+/- 0.05m)
    And @android:2 UWB azimuth to {A} is not available
    And @android:2 UWB elevation to {A} is not available

  Scenario: Multi-Controlee Ranging
    Given @adb has 3 attached devices
    And @netsim is running

    # Establish UWB sessions
    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with address B
    And @android:3 is a UWB CONTROLEE with address C

    # Controller starts ranging with two peers
    # Config 2 is used because Config 1 doesn't support one-to-many ranging.
    When @android:1 starts UWB ranging with config 2 and peers {B}, {C}
    And @android:2 starts UWB ranging with peer {A}
    And @android:3 starts UWB ranging with peer {A}

    Then @android:1 UWB peer {B} is connected
    Then @android:1 UWB peer {C} is connected

    Then @android:1 UWB distance to {B} is 0.0m (+/- 0.05m)
    And @android:1 UWB distance to {C} is 0.0m (+/- 0.05m)

  Scenario: UWB Ranging with Parameter Mismatch (Session ID)
    Given @adb has 2 attached devices
    And @netsim is running

    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with session S2 and address B

    When @android:1 starts UWB ranging with peer {B}
    And @android:2 starts UWB ranging with session {S2} and peer {A}

    # Session should enter "Active" state but never reach "Ranging"
    Then @android:2 UWB session state is Active
    And @android:2 UWB distance to {A} is not available

  Scenario: UWB Ranging with Movement Verification
    Given @adb has 2 attached devices
    And @netsim is running

    # Establish UWB sessions (explicitly configured to overwrite any leaking state)
    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with address B

    # Move @android:2 apart before starting to avoid (0,0,0) issues
    When @netsim moves @android:2 to 5.0, 0.0, 0.0

    # Using config 2 for robustness
    When @android:1 starts UWB ranging with config 2 and peer {B}
    And @android:2 starts UWB ranging with config 2 and peer {A}

    Then @android:2 UWB peer {A} is connected

    # Verify initial separation
    Then @android:1 UWB distance to {B} is 5.0m (+/- 0.1m)

    # Move @android:2 further away
    When @netsim moves @android:2 to 10.0, 0.0, 0.0
    Then @android:1 UWB distance to {B} is 10.0m (+/- 0.5m)

    # Move @android:2 to (10, 10, 0)
    When @netsim moves @android:2 to 10.0, 10.0, 0.0
    # distance = sqrt(10^2 + 10^2) = 14.14
    Then @android:1 UWB distance to {B} is 14.14m (+/- 0.5m)

  Scenario: UWB Lifecycle - Disable during session
    Given @adb has 2 attached devices
    And @netsim is running

    # Establish UWB sessions
    Given @android:1 is a UWB CONTROLLER with address A
    And @android:2 is a UWB CONTROLEE with address B

    When @android:1 starts UWB ranging with peer {B}
    And @android:2 starts UWB ranging with peer {A}
    Then @android:2 UWB peer {A} is connected

    # Disable UWB on @android:2
    When @android:2 disables UWB
    Then @android:2 UWB peer {A} is disconnected
