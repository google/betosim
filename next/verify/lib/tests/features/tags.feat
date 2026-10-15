Feature: Tags Support
  Tags (e.g., `@wip`, `@slow`) allow filtering scenarios execution.
  You can run only scenarios with specific tags or exclude them.
  Tags can be applied to both Scenarios and Features.

  Background:
    Tags are useful for filtering
    scenarios during execution.

    Given I organize my scenarios with tags

  @wip
  Scenario: Tagged scenario
    When I add 100
    Then result is 100

  Scenario: Untagged scenario
    When I add 1
    Then result is 1
