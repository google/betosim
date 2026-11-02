# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:p2p
Feature: Wi-Fi Direct (P2P)
  Reference: https://developer.android.com/reference/android/net/wifi/p2p/WifiP2pManager

  @nyt
  Scenario: P2P Peer Discovery
    Given two devices with P2P enabled
    When device A starts peer discovery
    Then device A should discover device B as a P2P peer

  @nyt
  Scenario: P2P Group Formation
    Given device A has discovered device B as a P2P peer
    When device A initiates a connection to device B
    Then they should negotiate Group Owner intent
    And a P2P group should be formed

  @nyt
  Scenario: P2P Service Discovery
    Given device A is advertising a "GameService" over P2P
    And device B has P2P enabled
    When device B starts P2P service discovery
    Then device B should discover "GameService" on device A without connecting

  @nyt
  Scenario: Connection Invitation
    Given device A is the owner of a P2P group
    And device B is in range and not in a group
    When device A invites device B to join the group
    Then device B should receive an invitation request

  @nyt
  Scenario: Persistent Group Reconnection
    Given device A and device B have previously formed a persistent P2P group
    When they come back into range and P2P is enabled
    Then they should automatically reconnect to the persistent group
