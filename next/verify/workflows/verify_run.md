---
description: How to run the Netsim e2e integration test suite using Bazel and Android Emulators manually.
---

# Netsim E2E Runner Workflow

When you need to run the `verify/runner:runner-e2e` integration tests locally, you must bring up the required daemons (`netsimd`) and Android Emulators, and ensure Bazel has the correct environment flags to communicate with the host's ADB daemon, bypassing the strict `linux-sandbox`.

To prevent flaky tests from lingering daemons, zombie emulators, or unauthenticated Wi-Fi states, all of this lifecycle management has been fully automated through a standardized wrapper script.

## 1. Run the E2E Lifecycle Wrapper
Simply execute the `verify_run.sh` wrapper script which will handle building `netsimd`, destroying stale background processes, launching fresh Android Emulators, explicitly awaiting complete network stack integration, and delegating to the `bazel test` suite:

```bash
# turbo-all
tools/netsim/next/verify/scripts/verify_run.sh
```

If it succeeds, your workspace environments are perfectly synchronized!

> [!TIP]
> The `scripts` directory in the `verify` package contains this highly robust `verify_run.sh` which employs `trap` functions to kill all underlying PIDs even if you CTRL-C mid-execution. It also manually commands `adb shell cmd wifi connect-network` to bypass `ApActor` cold-boot connection bugs.
