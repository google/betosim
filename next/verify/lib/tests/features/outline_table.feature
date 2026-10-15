Feature: Scenario Outline Table Replacement
  Scenario Outline: Test and replace values in data tables
    Given I reset the counter
    When I add <val1>
    And I check if additive table works:
      | key | value  |
      | add | <val2> |
    Then result is <sum>
    Examples:
      | val1 | val2 | sum |
      | 10   | 20   | 30  |
      | 5    | 5    | 10  |
