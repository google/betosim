Feature: Minimal VBS Example

  Scenario: Check Connected Devices
    # This step queries netsimd via gRPC (mocked in dry run) and checks if connected-devices exists.
    Then @netsim observes "connected-devices" should be ">=1"
