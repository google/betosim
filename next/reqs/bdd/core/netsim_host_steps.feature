# Copyright 2026 The Android Open Source Project

@ignore
Feature: Netsim Host Steps Control

  Scenario: Create and verify Beacon
    Given @netsim is running
    When Netsim creates BLE Beacon "TestBeacon" at 5.0, 5.0, 0.0
    Then Device "TestBeacon" in netsim is at 5.0, 5.0, 0.0

  Scenario: Move Beacon and verify
    Given @netsim is running
    When Netsim creates BLE Beacon "MovingBeacon" at 0.0, 0.0, 0.0
    And @netsim moves @MovingBeacon to 10.0, 10.0, 0.0
    Then Device "MovingBeacon" in netsim is at 10.0, 10.0, 0.0

  Scenario: Create and verify Access Point
    Given @netsim is running
    When Netsim creates Wi-Fi Access Point "TestAP" with protocol "ax"
    Then Wi-Fi Access Point "TestAP" exists in netsim
    And Wi-Fi Access Point "TestAP" in netsim has protocol "ax"

  Scenario: Create and remove multiple Access Points
    Given @netsim is running
    When Netsim creates Wi-Fi Access Point "AP1" with protocol "ax"
    And Netsim creates Wi-Fi Access Point "AP2" with protocol "ax"
    Then Wi-Fi Access Point "AP1" exists in netsim
    And Wi-Fi Access Point "AP2" exists in netsim
    When Netsim removes Wi-Fi Access Point "AP1"
    Then Wi-Fi Access Point "AP2" exists in netsim
    And Wi-Fi Access Point "AP1" does not exist in netsim

  Scenario: Verify Netsim version and device count
    Given @netsim is running
    When Netsim creates BLE Beacon "Beacon1" at 1.0, 1.0, 0.0
    And Netsim creates BLE Beacon "Beacon2" at 2.0, 2.0, 0.0
    Then Netsim has at least 2 devices

  Scenario: Verify Device Position with custom delta
    Given @netsim is running
    When Netsim creates BLE Beacon "DeltaBeacon" at 1.0, 1.0, 0.0
    Then Device "DeltaBeacon" in netsim is at 1.0005, 1.0005, 0.0005 with delta 0.001


