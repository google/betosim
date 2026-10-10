# Network Simulator Development

This section walks you through building netsim from source.

Netsim can be built as part of emulator or cuttlefish and best practice is to
setup both and switch between repo directories to test each build environment.

* To build with emulator, follow the [netsim with emulator](#netsim_with_emulator)
section to build netsim by `cmake` in `emu-master-dev` and `netsim-dev` manifest branch.

* To build with cuttlefish, follow the [netsim with
cuttlefish](#netsim_with_cuttlefish) to build netsim by `soong` in `aosp-main`
manifest branch.

## Emulator and cuttlefish build branches

The *netsim* network simulator is built as a component of
[emulator](https://source.android.com/docs/setup/create/avd)
and
[cuttlefish](https://source.android.com/docs/setup/create/cuttlefish)
virtual devices.

*Emulator* allows you to run emulations of Android devices on Windows, macOS or
Linux machines. Emulator runs the Android operating system in a virtual machine
called an Android Virtual Device (AVD).
The emulator is typically used from
[Android Studio](https://developer.android.com/studio).

*Cuttlefish* is a configurable virtual Android device that can be run on Linux
x86 machines both remotely (using third-party cloud offerings such as Google
Cloud Engine) and locally. Cuttlefish runs the Android operating system in a
virtual machine called a Cuttlefish Virtual Device (CVD).
Cuttlefish is typically used by developers working with AOSP code to [launch
AOSP builds](https://source.android.com/docs/setup/create/cuttlefish-use).

The table below summarizes the two virtual device environments:

|                 |      emulator                        | cuttlefish         |
|:----------------|:------------------------------------:|:----------------:  |
| AOSP branch     | `emu-master-dev` & `netsim-dev`      | `aosp-main`      |
| launcher        | `emulator` app and<br>Android Studio | `launch_cvd` and<br>`cvd` app |
| best for        | App developer         | Platform developer |
| Supported OS    | Linux, MacOS, Windows | Linux              |
| Build system    | &nbsp;&nbsp; CMake (CMakeLists.txt) &nbsp;&nbsp; | &nbsp;&nbsp; Soong (Android.bp) &nbsp;&nbsp; |
| Virtual device  | AVD                   | CVD                |

Netsim is the default networking backplane for AVD and CVD emulated Android
devices.

## <a name="netsim_with_emulator"></a>Build netsim with emulator

For developing netsim alongside emulator, start with the OS specific build
instructions:
* [Android emulator Windows Development](
https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-master-dev/android/docs/WINDOWS-DEV.md
)
* [Android emulator MacOS Development](
https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-master-dev/android/docs/DARWIN-DEV.md
)
* [Android emulator Linux Development](
https://android.googlesource.com/platform/external/qemu/+/refs/heads/emu-master-dev/android/docs/LINUX-DEV.md
)

In general changes should be built and tested on all three operating systems.

Follow the instructions above links for workstation setup.

### Initialize and sync the code

Download the emu-master-dev branch:

```
mkdir /repo/emu-master-dev; cd /repo/emu-master-dev
repo init -u https://android.googlesource.com/platform/manifest -b emu-master-dev
```
Sync the source code:

```
repo sync -j8
```

### Emulator full build

Use Android emulator toolchain script to run the build:
```
cd /repo/emu-master-dev/external/qemu
sh android/rebuild.sh --gfxstream
```

The output can be found in:
```
/repo/emu-master-dev/external/qemu/objs/distribution/emulator
```

### Netsim incremental build

Currently the netsim binaries in
`/repo/emu-master-dev/prebuilts/android-emulator-build/common/netsim/*` does get weekly updates with the latest binary. If you want to build netsim from source, you must sync and build from a separate branch `netsim-dev`.

Download the netsim-dev branch:

```
mkdir /repo/netsim-dev; cd /repo/netsim-dev
repo init -u https://android.googlesource.com/platform/manifest -b netsim-dev
```
Sync the source code:

```
repo sync -j8
```

The `emulator` rebuild script does a complete clean build of all emulator components.
For incremental builds of the `netsimd` component, you can use the `build_tools.py` script.

By default, it uses **Bazel** to build:
```
cd /repo/netsim-dev/tools/netsim
scripts/build_tools.py --task compileinstall
```

To clean the build artifacts (Bazel `clean --expunge`):
```
scripts/build_tools.py --clean
```

To build with **CMake** (legacy), use the `--cmake` flag:
```
scripts/build_tools.py --cmake --task configure compileinstall
```

The output can be found in:

```
/repo/netsim-dev/tools/netsim/objs/distribution/emulator
```

You can copy the netsim binaries into `emu-master-dev`:

```
cp -r /repo/netsim-dev/tools/netsim/objs/distribution/emulator/* /repo/emu-master-dev/external/qemu/objs/distribution/emulator
```

## <a name="netsim_with_cuttlefish"></a>Build netsim with cuttlefish

The [Android Developer Codelab](https://source.android.com/docs/setup/start)
provides instructions for building and running cuttlefish AVDs.

Follow the instructions in the codelab for workstation setup.

### Initialize and sync the code

Initialize the repo:
```
mkdir /repo/aosp-main; cd /repo/aosp-main
repo init -u https://android.googlesource.com/platform/manifest -b aosp-main
```

Sync the source code:
```
repo sync -j8
```

### Build cuttlefish

Set up build environment:
```
source build/envsetup.sh
```

Set the target device type:
```
lunch aosp_cf_x86_64_phone
```

Start the build:
```
m -j64
```

The netsim executable can be found in:
```
/repo/aosp-main/out/host/linux-x86/bin
```

### Cuttlefish incremental netsim build


Start the build with netsimd target:
```
m netsimd -j64
```

## Unit Testing

Unit tests can be run from the `aosp-main` branch using the `atest` command:
```
atest --host-unit-test-only --test-filter netsim
```

Unit tests can be run from the `netsim-dev` branch using the following command
```
scripts/build_tools.py --task test
```

### Repo workflow

The repo workflow for creating and uploading a change request:
```
repo start new-branch
git add <files>
git commit
repo upload --branch=new-branch
```

Subsequent commits:
```
git add <files>
git commit --amend --no-edit
repo upload --branch=new-branch
```

## Documentation

The developer and user documentation for netsim is stored in the `guide`
directory in `mdbook` format.  Refer to
[install](https://rust-lang.github.io/mdBook/guide/installation.html)
for instructions on how to install `mdbook`.

Use this command to start a local web server with the netsim guide:
```
mdbook serve guide
```

