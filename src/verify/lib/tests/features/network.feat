Feature: Network Packet Simulation
  As a network developer
  I want to verify packet handling
  So that I can ensure protocol compliance

  Scenario: UDP Packet Exchange
    Given I have a UDP Echo Server on Port 7
    When I send a UDP packet with:
      | udp.srcport  | 12345       |
      | udp.dstport  | 7           |
      | udp.length   | 8           |
      | udp.checksum | 0           |
    Then I receive a UDP packet matching:
      | udp.srcport  | 12345       |
      | udp.dstport  | 7           |
