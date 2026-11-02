# Copyright 2026 The Android Open Source Project

@epic:wifi @feature:wifi_service_discovery
Feature: WiFi Service Discovery

  @nyt
  Scenario: mDNS Loopback
    Given @adb has 1 attached device
    When @android:1 advertises service _http._tcp. as MyLoopbackService
    And @android:1 starts discovery for _http._tcp.
    Then @android:1 finds service MyLoopbackService

  @nyt
  Scenario: mDNS/NSD Cross-Device
    Given @adb has 2 attached devices
    When @android:1 advertises service _http._tcp. as MyWiFiService
    And @android:2 starts discovery for _http._tcp.
    Then @android:2 finds service MyWiFiService

  @nyt
  Scenario: Unregister service stops discovery
    Given @adb has 2 attached devices
    And @android:1 advertises service _http._tcp. as MyWiFiService
    And @android:2 has discovered service MyWiFiService
    When @android:1 unregisters service MyWiFiService
    Then @android:2 should detect that service MyWiFiService is lost

  @nyt
  Scenario: Resolve discovered service
    Given @adb has 2 attached devices
    And @android:1 advertises service _http._tcp. as MyWiFiService
    And @android:2 has discovered service MyWiFiService
    When @android:2 resolves service MyWiFiService
    Then @android:2 should receive the IP address and port for MyWiFiService
