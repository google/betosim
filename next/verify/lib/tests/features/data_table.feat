Feature: Data Table Test
  Data Tables are a convenient way to pass list or tabular data
  to a step definition. This structure is automatically converted
  into a `DataTable` struct in Rust step definitions.

  Scenario: Users
    Given the following users:
      | name  | age |
      | Alice | 30  |
      | Bob   | 25  |
    Then lookup Alice is 30
    And lookup Bob is 25
