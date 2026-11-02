Feature: Background Data Table Support
  Background: Initializing users in background
    Given I setup with default users:
      | name  | age |
      | Admin | 99  |
      | Guest | 10  |
  Scenario: Verify background users
    Then lookup Admin is 99
    And lookup Guest is 10
