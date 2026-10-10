#!/usr/bin/env python3
# Copyright 2024 The Android Open Source Project
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

#
import argparse
import csv
import datetime
import os
import platform
import subprocess
import time
import psutil
import requests

PLATFORM_SYSTEM = platform.system().lower()
QEMU_ARCH_MAP = {'arm64': 'aarch64', 'AMD64': 'x86_64'}
PLATFORM_MACHINE = QEMU_ARCH_MAP.get(platform.machine(), platform.machine())
TEST_DURATION = 300
CURRENT_PATH = os.path.dirname(os.path.abspath(__file__))
EXE_SUFFIX = '.exe' if PLATFORM_SYSTEM == 'windows' else ''
NETSIMD_BINARY = f'netsimd{EXE_SUFFIX}'
NETSIM_FRONTEND_HTTP_URI = 'http://localhost:7681'
EMULATOR_BINARY = f'emulator{EXE_SUFFIX}'
QEMU_SYSTEM_BINARY = f'qemu-system-{PLATFORM_MACHINE}{EXE_SUFFIX}'


def _get_cpu_usage():
  """Retrieves CPU and memory usage for netsimd and qemu."""
  netsimd_usage, qemu_usage = [], []

  for process in psutil.process_iter(
      ['name', 'cpu_percent', 'num_threads', 'memory_info']
  ):
    process_name = process.info['name']
    if process_name == NETSIMD_BINARY:
      netsimd_usage.append(process.info)
    elif process_name == QEMU_SYSTEM_BINARY:
      qemu_usage.append(process.info)

  def _validate_and_extract(process_list, process_name):
    if len(process_list) > 1:
      raise LookupError(f'Multiple {process_name} processes found')
    if not process_list:
      raise LookupError(f'Process {process_name} not found')
    return process_list[0]

  netsimd_info = _validate_and_extract(netsimd_usage, NETSIMD_BINARY)
  qemu_info = _validate_and_extract(qemu_usage, QEMU_SYSTEM_BINARY)

  return (
      netsimd_info['cpu_percent'],
      qemu_info['cpu_percent'],
      netsimd_info['num_threads'],
      netsimd_info['memory_info'].rss / 1024 / 1024,
  )


def _process_usage_iteration(writer, avd, netsim_wifi, iteration):
  """Collects and writes usage data for a single iteration."""
  try:
    netsimd_cpu, qemu_cpu, netsimd_threads, netsimd_mem = _get_cpu_usage()
    if iteration == 0:
      time.sleep(0.1)
      return
    data = [time.time(), netsimd_cpu, qemu_cpu, netsimd_threads, netsimd_mem]
    if netsim_wifi:
      data.extend(_get_wifi_packet_count(avd))
    print(f'Got {data}')
    writer.writerow(data)
  except LookupError as e:
    print(e)
    time.sleep(1)
  time.sleep(1)


def _trace_usage(filename: str, avd: str, netsim_wifi: bool):
  """Traces usage data and writes to a CSV file."""
  with open(filename, 'w', newline='') as csvfile:
    writer = csv.writer(csvfile)
    headers = [
        'Timestamp',
        NETSIMD_BINARY,
        QEMU_SYSTEM_BINARY,
        'NetSimThreads',
        'NetSimMemUsage(MB)',
    ]
    if netsim_wifi:
      headers.extend(['txCount', 'rxCount'])
    writer.writerow(headers)
    for i in range(TEST_DURATION):
      _process_usage_iteration(writer, avd, netsim_wifi, i)


def _launch_emulator(cmd):
  """Utility function for launching Emulator"""
  if PLATFORM_SYSTEM == 'windows':
    return subprocess.Popen(
        cmd,
        creationflags=subprocess.CREATE_NEW_PROCESS_GROUP,
    )
  else:
    return subprocess.Popen(cmd)


def _terminate_emulator(process):
  """Utility function for terminating Emulator"""
  try:
    if PLATFORM_SYSTEM == 'windows':
      import signal

      process.send_signal(signal.CTRL_BREAK_EVENT)
      process.wait()
    else:
      process.terminate()
  except OSError:
    print('Process already termianted')


def _get_wifi_packet_count(avd: str):
  """Utility function for getting WiFi Packet Counts.

  Returns (txCount, rxCount)
  """
  avd = avd.replace('_', ' ')
  try:
    response = requests.get(NETSIM_FRONTEND_HTTP_URI + '/v1/devices')
    response.raise_for_status()
    for device in response.json()['devices']:
      if device['name'] == avd:
        for chip in device['chips']:
          if chip['kind'] == 'WIFI':
            return (chip['wifi']['txCount'], chip['wifi']['rxCount'])
  except requests.exceptions.RequestException as e:
    print(f'Request Error: {e}')
  except KeyError as e:
    print(f'KeyError: {e}')
  except IndexError as e:
    print(f'IndexError: {e}')
  return (0, 0)


def _collect_cpu_usage(avd: str, netsim_wifi: bool):
  """Utility function for running the CPU usage collection session"""
  # Setup cmd and filename to trace
  time_now = datetime.datetime.now().strftime('%Y-%m-%d-%H-%M-%S')
  cmd = [f'{CURRENT_PATH}/{EMULATOR_BINARY}', '-avd', avd, '-wipe-data']
  filename = (
      f'netsimd_cpu_usage_{PLATFORM_SYSTEM}_{PLATFORM_MACHINE}_{time_now}.csv'
  )
  if netsim_wifi:
    cmd.extend(['-feature', 'WiFiPacketStream'])
    filename = f'netsimd_cpu_usage_{PLATFORM_SYSTEM}_{PLATFORM_MACHINE}_WiFiPacketStream_{time_now}.csv'

  # Launch emulator
  process = _launch_emulator(cmd)

  # Enough time for Emulator to boot
  time.sleep(10)

  # Trace CPU usage
  _trace_usage(filename, avd, netsim_wifi)

  # Terminate Emulator Process
  _terminate_emulator(process)


def main():
  # Check if ANDROID_SDK_ROOT env is defined
  if 'ANDROID_SDK_ROOT' not in os.environ:
    print('Please set ANDROID_SDK_ROOT')
    return

  # Check if Emulator Binary exists
  emulator_path = f'{CURRENT_PATH}/{EMULATOR_BINARY}'
  if not os.path.isfile(emulator_path):
    print(
        f"Can't find {emulator_path}. Please place the file with the binaries"
        ' before executing.'
    )
    return

  # Set avd provided by the user
  parser = argparse.ArgumentParser()
  parser.add_argument('avd', help='The AVD to use', type=str)
  args = parser.parse_args()

  # Collect CPU usage without netsim WiFi
  _collect_cpu_usage(args.avd, False)

  # Enough time for Emulator to terminate
  time.sleep(10)

  # Collect CPU usage with netsim WiFi
  _collect_cpu_usage(args.avd, True)

  print('CPU Usage Completed!')


if __name__ == '__main__':
  main()
