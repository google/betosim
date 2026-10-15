# Copyright 2026 The Android Open Source Project

Feature: Feature Observables Verification

  @verify_observed:mock-feature
  Scenario: Verify mock feature counter with tags
    Given @adb has 1 attached device
    When @avd fetch feature observables
