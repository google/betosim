Feature: Basic Calculator Operations
  Scenario: Reset and simple addition
    Given I reset the counter
    When I add 10
    Then result is 10
