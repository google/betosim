Feature: Scenario Outline Support
  Outlines allow running the same scenario multiple times
  with different values. This reduces duplication and makes tests
  more expressive by separating logic from data.

  Background:
    The background step ensures we have a clean slate.

    Given I have a calculator with No Memory

  Scenario Outline: Add multiple numbers
    When I add <num1>
    And I add <num2>
    Then result is <total>

    Examples:
      | num1 | num2 | total |
      | 10   | 20   | 30    |
      | 1    | 2    | 3     |
