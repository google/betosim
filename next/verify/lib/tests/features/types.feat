Feature: Type Support
  Cuke supports automatic argument parsing for primitive types
  including `bool`, `f64`, `i32`, and others. This ensures type
  safety at the boundary between Gherkin steps and Rust code.

  Scenario: Boolean and Float types
    Given system is true
    When I set temperature to 98.6
    Then system should be true
    And temperature should be 98.6

  Scenario: Disable system
    Given system is false
    Then system should be false
