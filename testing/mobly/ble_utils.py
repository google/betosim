# Copyright 2026 The Android Open Source Project
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

"""BLE test utils for netsim."""

import logging
import time
from typing import Any

from mobly import asserts
from mobly import utils
from mobly.controllers import android_device
from mobly.snippet import callback_event

# Number of seconds for the target to stay BLE advertising.
ADVERTISING_TIME = 120
# Number of seconds for the target to start BLE advertising.
ADVERTISING_START_TIME = 30
# The number of seconds to wait for receiving scan results.
SCAN_TIMEOUT = 20
# The number of seconds to wair for connection established.
CONNECTION_TIMEOUT = 60
# The number of seconds to wait before cancel connection.
CANCEL_CONNECTION_WAIT_TIME = 0.1
# UUID for test service.
TEST_BLE_SERVICE_UUID = '0000fe23-0000-1000-8000-00805f9b34fb'
# UUID for write characteristic.
TEST_WRITE_UUID = '0000e632-0000-1000-8000-00805f9b34fb'
# UUID for second write characteristic.
TEST_SECOND_WRITE_UUID = '0000e633-0000-1000-8000-00805f9b34fb'
# UUID for read test.
TEST_READ_UUID = '0000e631-0000-1000-8000-00805f9b34fb'
# UUID for second read characteristic.
TEST_SECOND_READ_UUID = '0000e634-0000-1000-8000-00805f9b34fb'
# UUID for third read characteristic.
TEST_THIRD_READ_UUID = '0000e635-0000-1000-8000-00805f9b34fb'
# UUID for scan response.
TEST_SCAN_RESPONSE_UUID = '0000e639-0000-1000-8000-00805f9b34fb'
# Advertise settings in json format for Ble Advertise.
ADVERTISE_SETTINGS = {
    'AdvertiseMode': 'ADVERTISE_MODE_LOW_LATENCY',
    'Timeout': ADVERTISING_TIME * 1000,
    'Connectable': True,
    'TxPowerLevel': 'ADVERTISE_TX_POWER_ULTRA_LOW',
}
# Ramdom data to represent device stored in advertise data.
DATA = utils.rand_ascii_str(16)
# Random data for scan response.
SCAN_RESPONSE_DATA = utils.rand_ascii_str(16)
# Random data for read operation.
READ_DATA = utils.rand_ascii_str(8)
# Random data for second read operation.
SECOND_READ_DATA = utils.rand_ascii_str(8)
# Random data for third read operation.
THIRD_READ_DATA = utils.rand_ascii_str(8)
# Random data for write operation.
WRITE_DATA = utils.rand_ascii_str(8)
# Random data for second write operation.
SECOND_WRITE_DATA = utils.rand_ascii_str(8)
# Advertise data in json format for BLE advertise.
ADVERTISE_DATA = {
    'IncludeDeviceName': False,
    'ServiceData': [{'UUID': TEST_BLE_SERVICE_UUID, 'Data': DATA}],
}
# Advertise data in json format representing scan response for BLE advertise.
SCAN_RESPONSE = {
    'IncludeDeviceName': False,
    'ServiceData': [{
        'UUID': TEST_SCAN_RESPONSE_UUID,
        'Data': SCAN_RESPONSE_DATA,
    }],
}
# Scan filter in json format for BLE scan.
SCAN_FILTER = {'ServiceUuid': TEST_BLE_SERVICE_UUID}
# Scan settings in json format for BLE scan.
SCAN_SETTINGS = {'ScanMode': 'SCAN_MODE_LOW_LATENCY'}
# Characteristics for write in json format.
WRITE_CHARACTERISTIC = {
    'UUID': TEST_WRITE_UUID,
    'Property': 'PROPERTY_WRITE',
    'Permission': 'PERMISSION_WRITE',
}
SECOND_WRITE_CHARACTERISTIC = {
    'UUID': TEST_SECOND_WRITE_UUID,
    'Property': 'PROPERTY_WRITE',
    'Permission': 'PERMISSION_WRITE',
}
# Characteristics for read in json format.
READ_CHARACTERISTIC = {
    'UUID': TEST_READ_UUID,
    'Property': 'PROPERTY_READ',
    'Permission': 'PERMISSION_READ',
    'Data': READ_DATA,
}
SECOND_READ_CHARACTERISTIC = {
    'UUID': TEST_SECOND_READ_UUID,
    'Property': 'PROPERTY_READ',
    'Permission': 'PERMISSION_READ',
    'Data': SECOND_READ_DATA,
}
THIRD_READ_CHARACTERISTIC = {
    'UUID': TEST_THIRD_READ_UUID,
    'Property': 'PROPERTY_READ',
    'Permission': 'PERMISSION_READ',
    'Data': THIRD_READ_DATA,
}
# Service data in json format for Ble Server.
SERVICE = {
    'UUID': TEST_BLE_SERVICE_UUID,
    'Type': 'SERVICE_TYPE_PRIMARY',
    'Characteristics': [
        WRITE_CHARACTERISTIC,
        SECOND_WRITE_CHARACTERISTIC,
        READ_CHARACTERISTIC,
        SECOND_READ_CHARACTERISTIC,
        THIRD_READ_CHARACTERISTIC,
    ],
}
# Macros for literal string.
UUID = 'UUID'
GATT_SUCCESS = 'GATT_SUCCESS'
STATE = 'newState'
STATUS = 'status'


def IsRequiredScanResult(scan_result: callback_event.CallbackEvent) -> bool:
  result = scan_result.data['result']
  for service in result['ScanRecord']['Services']:
    if service[UUID] == TEST_BLE_SERVICE_UUID and service['Data'] == DATA:
      return True
  return False


def Discover(
    scanner: android_device.AndroidDevice,
    advertiser: android_device.AndroidDevice,
) -> dict[str, Any]:
  """Logic for BLE scan and advertising.

  Steps:
    1. Advertiser starts advertising and gets a startSuccess callback.
    2. Scanner starts scanning and finds advertiser from scan results.

  Verifies:
    Advertiser is discovered within 5s by scanner.

  Args:
    scanner: AndroidDevice. The device that starts BLE scan to find target.
    advertiser: AndroidDevice. The device that keeps advertising so other
      devices acknowledge it.

  Returns:
    dict. Scan results.

  Raises:
    TimeoutError: The expected event does not occur within the time limit.
  """
  # Retry initial command in case command is lost after triggering a reset
  max_attempts = 2
  for attempt_num in range(max_attempts):
    advertiser.advertise_callback = advertiser.mbs.bleStartAdvertising(
        ADVERTISE_SETTINGS, ADVERTISE_DATA, SCAN_RESPONSE
    )
    scanner.scan_callback = scanner.mbs.bleStartScan(
        [SCAN_FILTER], SCAN_SETTINGS
    )
    success = False
    for _ in range(ADVERTISING_START_TIME):
      failure = advertiser.advertise_callback.getAll('onStartFailure')
      if failure:
        logging.warning(
            "'onStartFailure' event detected after bleStartAdvertising"
        )
      success = advertiser.advertise_callback.getAll('onStartSuccess')
      if success:
        break
      time.sleep(1)
    else:
      logging.error(
          'Timed out after %ss waiting for an "onStartSuccess" event ',
          ADVERTISING_START_TIME,
      )
    if not success:
      if attempt_num < max_attempts - 1:
        logging.warning(
            "'onStartSuccess' event was not received after "
            'bleStartAdvertising. Retrying... (%d)',
            attempt_num + 1,
        )
      else:
        raise TimeoutError(
            f'Timed out after {max_attempts} retries of '
            f'{ADVERTISING_START_TIME}s waiting for an '
            '"onStartSuccess" event '
        )

  advertiser.log.info('BLE advertising started')
  time.sleep(SCAN_TIMEOUT)
  scan_result = scanner.scan_callback.waitForEvent(
      'onScanResult', IsRequiredScanResult, SCAN_TIMEOUT
  )
  scan_success = False
  scan_response_found = False
  result = scan_result.data['result']
  scan_start_to_result_time_ms = scan_result.data['StartToResultTimeDeltaMs']
  for service in result['ScanRecord']['Services']:
    if service[UUID] == TEST_BLE_SERVICE_UUID and service['Data'] == DATA:
      scanner.connect_to_address = result['Device']['Address']
      scan_success = True
    if (
        service[UUID] == TEST_SCAN_RESPONSE_UUID
        and service['Data'] == SCAN_RESPONSE_DATA
    ):
      scan_response_found = True
  asserts.assert_true(
      scan_success, 'Advertiser is not found inside %d seconds' % SCAN_TIMEOUT
  )
  asserts.assert_true(scan_response_found, 'Scan response is not found')
  logging.info('Discovery metrics: %d', scan_start_to_result_time_ms)
  return result


def StartScanning(
    scanner: android_device.AndroidDevice, scan_duration: int
) -> list[dict[str, Any]]:
  """Logic for BLE scanning for advertisers.

  Steps:
    1. Scanner starts scanning with retries
    2. Retrieves the ScanResult

  Verifies:
    Advertiser is discovered within timeout by scanner.

  Args:
    scanner: AndroidDevice. The device that starts BLE scan to find advertisers.
    scan_duration: Number of seconds for each scan attempt

  Returns:
    List of dicts containing Scan results.

  Raises:
    TimeoutError: The expected event does not occur within the time limit.
  """
  # Retry initial command in case command is lost after triggering a reset
  max_attempts = 3
  scan_success = False
  result = []
  scan_result = None
  for attempt_num in range(max_attempts):
    scanner.scan_callback = scanner.mbs.bleStartScan()
    scanner.log.info('BLE scanning started')
    failure = scanner.scan_callback.getAll('onScanFailed')
    if failure:
      logging.warning("'onScanFailed' event detected after bleStartScan")
      continue
    success = False
    for _ in range(int(SCAN_TIMEOUT / scan_duration)):
      time.sleep(scan_duration)
      scan_result = scanner.scan_callback.getAll('onScanResult')
      if scan_result:
        success = True
        break
    else:
      logging.error(
          'Timed out after %ss waiting for an "onScanResult" event ',
          SCAN_TIMEOUT,
      )
    if success:
      break
    if attempt_num < max_attempts - 1:
      logging.warning(
          "'onScanResult' event was not received after "
          'bleStartScan. Retrying... (%d)',
          attempt_num + 1,
      )
    else:
      raise TimeoutError(
          f'Timed out after {max_attempts} retries of '
          f'{SCAN_TIMEOUT}s waiting for an '
          '"onScanResult" event '
      )

  if scan_result:
    scan_success = True
    result = [result.data['result'] for result in scan_result]

  asserts.assert_true(
      scan_success, 'Advertiser is not found inside %d seconds' % SCAN_TIMEOUT
  )
  return result


def StopDiscover(
    scanner: android_device.AndroidDevice,
    advertiser: android_device.AndroidDevice,
) -> None:
  """Logic for stopping BLE scan and advertising.

  Steps:
    1. Scanner stops scanning.
    2. Advertiser stops advertising.

  Args:
    scanner: AndroidDevice. The device that starts BLE scan to find target.
    advertiser: AndroidDevice. The device that keeps advertising so other
      devices acknowledge it.
  """
  scanner.mbs.bleStopScan(scanner.scan_callback.callback_id)
  scanner.log.info('BLE scanning stopped')
  advertiser.mbs.bleStopAdvertising(advertiser.advertise_callback.callback_id)
  advertiser.log.info('BLE advertising stopped')


def StopScanning(scanner: android_device.AndroidDevice) -> None:
  """Logic for stopping BLE scan.

  Steps:
    1. Scanner stops scanning.

  Args:
    scanner: AndroidDevice. The device that starts BLE scan to find target.
  """
  scanner.mbs.bleStopScan(scanner.scan_callback.callback_id)
  scanner.log.info('BLE scanning stopped')


def Connect(
    client: android_device.AndroidDevice, server: android_device.AndroidDevice
) -> None:
  """Logic for create a Gatt connection between a client and a server.

  Steps:
    1. Server starts and service added properly.
    2. Client connects to server via Gatt, connection completes with
    GATT_SUCCESS within TIMEOUT, onConnectionStateChange/STATE_CONNECTED is
    called EXACTLY once.

  Verifies:
    Both the client and the server consider themselves connected to each other.

  Args:
    client: AndroidDevice. The device that behaves as GATT client.
    server: AndroidDevice. The device that behaves as GATT server.
  """
  server.server_callback = server.mbs.bleStartServer([SERVICE])
  start_server_result = server.server_callback.waitAndGet('onServiceAdded', 30)
  asserts.assert_equal(start_server_result.data[STATUS], GATT_SUCCESS)
  uuids = [
      characteristic[UUID]
      for characteristic in start_server_result.data['Service'][
          'Characteristics'
      ]
  ]
  for uuid in [
      characteristic[UUID] for characteristic in SERVICE['Characteristics']
  ]:
    asserts.assert_true(uuid in uuids, 'Failed to find uuid %s.' % uuid)
  server.log.info('BLE server started')
  client.client_callback = client.mbs.bleConnectGatt(client.connect_to_address)
  start_client_result = client.client_callback.waitAndGet(
      'onConnectionStateChange', CONNECTION_TIMEOUT
  )
  extra_events = client.client_callback.getAll('onConnectionStateChange')
  asserts.assert_false(
      extra_events,
      'Got unexpected onConnectionStateChange events: %s',
      extra_events,
  )
  asserts.assert_equal(start_client_result.data[STATUS], GATT_SUCCESS)
  asserts.assert_equal(start_client_result.data[STATE], 'STATE_CONNECTED')
  client.log.info('BLE client connected')
  # Verify that the server side also considers itself connected.
  server_event = server.server_callback.waitAndGet('onConnectionStateChange')
  asserts.assert_equal(server_event.data[STATUS], GATT_SUCCESS)
  asserts.assert_equal(
      server_event.data[STATE],
      'STATE_CONNECTED',
      'The server side does not consider itself connected, error!',
  )
  logging.info('Gatt connection complete.')
  logging.info(
      'Connection metrics: %d', start_client_result.data['gattConnectionTimeMs']
  )


def Disconnect(
    client: android_device.AndroidDevice, server: android_device.AndroidDevice
) -> None:
  """Logic for stopping BLE client and server.

  Steps:
    1. Client calls disconnect, gets a callback with STATE_DISCONNECTED and
    GATT_SUCCESS.
    2. Server closes.

  Verifies: Client gets corresponding callback.

  Args:
    client: AndroidDevice. The device that behaves as GATT client.
    server: AndroidDevice. The device that behaves as GATT server.
  """
  client.mbs.bleDisconnect()
  stop_client_result = client.client_callback.waitAndGet(
      'onConnectionStateChange', 30
  )
  asserts.assert_equal(stop_client_result.data[STATUS], GATT_SUCCESS)
  asserts.assert_equal(stop_client_result.data[STATE], 'STATE_DISCONNECTED')
  client.log.info('BLE client disconnected')
  server.mbs.bleStopServer()
  server.log.info('BLE server stopped')


def DiscoverServices(client: android_device.AndroidDevice) -> None:
  """Logic for BLE services discovery.

  Steps:
    1. Client successfully completes service discovery & gets
    onServicesDiscovered callback within some TIMEOUT, onServicesDiscovered/
    GATT_SUCCESS is called EXACTLY once.
    2. Client discovers the readable and writable characteristics.

  Verifies:
    Client gets corresponding callback.

  Args:
    client: AndroidDevice. The device that behaves as GATT client.
  """
  client.mbs.bleDiscoverServices()
  time.sleep(CONNECTION_TIMEOUT)
  discover_services_results = client.client_callback.getAll(
      'onServiceDiscovered'
  )
  asserts.assert_equal(len(discover_services_results), 1)
  service_discovered = False
  asserts.assert_equal(discover_services_results[0].data[STATUS], GATT_SUCCESS)
  for service in discover_services_results[0].data['Services']:
    if service['UUID'] == TEST_BLE_SERVICE_UUID:
      service_discovered = True
      uuids = [
          characteristic[UUID] for characteristic in service['Characteristics']
      ]
      for uuid in [
          characteristic[UUID] for characteristic in SERVICE['Characteristics']
      ]:
        asserts.assert_true(uuid in uuids, 'Failed to find uuid %s.' % uuid)
  asserts.assert_true(
      service_discovered, 'Failed to discover the customize service'
  )
  client.log.info('BLE discover services finished')


def ReadCharacteristic(client: android_device.AndroidDevice) -> None:
  """Logic for BLE characteristic retrieval.

  Steps:
    1. Client reads a characteristic from server & gets true.
    2. Server calls sendResponse & client gets onCharacteristicRead.

  Verifies:
    Client gets corresponding callback.

  Args:
    client: AndroidDevice. The device that behaves as GATT client.
  """
  read_operation_result = client.mbs.bleReadOperation(
      TEST_BLE_SERVICE_UUID, TEST_READ_UUID
  )
  asserts.assert_true(
      read_operation_result, 'BLE read operation failed to start'
  )
  read_operation_result = client.client_callback.waitAndGet(
      'onCharacteristicRead', 30
  )
  asserts.assert_equal(read_operation_result.data[STATUS], GATT_SUCCESS)
  asserts.assert_equal(read_operation_result.data['Data'], READ_DATA)
  client.log.info('Read operation finished')
  read_operation_result = client.mbs.bleReadOperation(
      TEST_BLE_SERVICE_UUID, TEST_SECOND_READ_UUID
  )
  asserts.assert_true(
      read_operation_result, 'BLE read operation failed to start'
  )
  read_operation_result = client.client_callback.waitAndGet(
      'onCharacteristicRead', 30
  )
  asserts.assert_equal(read_operation_result.data[STATUS], GATT_SUCCESS)
  asserts.assert_equal(read_operation_result.data['Data'], SECOND_READ_DATA)
  client.log.info('Second read operation finished')
  read_operation_result = client.mbs.bleReadOperation(
      TEST_BLE_SERVICE_UUID, TEST_THIRD_READ_UUID
  )
  asserts.assert_true(
      read_operation_result, 'BLE read operation failed to start'
  )
  read_operation_result = client.client_callback.waitAndGet(
      'onCharacteristicRead', 30
  )
  asserts.assert_equal(read_operation_result.data[STATUS], GATT_SUCCESS)
  asserts.assert_equal(read_operation_result.data['Data'], THIRD_READ_DATA)
  client.log.info('Third read operation finished')


def WriteCharacteristic(
    client: android_device.AndroidDevice, server: android_device.AndroidDevice
) -> None:
  """Logic for BLE characteristic write.

  Steps:
    1. Client writes a characteristic to server & gets true.
    2. Server calls sendResponse & client gets onCharacteristicWrite.

  Verifies:
    Client gets corresponding callback.

  Args:
    client: AndroidDevice. The device that behaves as GATT client.
    server: AndroidDevice. The device that behaves as GATT server.
  """
  write_operation_result = client.mbs.bleWriteOperation(
      TEST_BLE_SERVICE_UUID, TEST_WRITE_UUID, WRITE_DATA
  )
  asserts.assert_true(
      write_operation_result, 'BLE write operation failed to start'
  )
  server_write_operation_result = server.server_callback.waitAndGet(
      'onCharacteristicWriteRequest', 30
  )
  asserts.assert_equal(server_write_operation_result.data['Data'], WRITE_DATA)
  client.client_callback.waitAndGet('onCharacteristicWrite', 30)
  client.log.info('Write operation finished')
  write_operation_result = client.mbs.bleWriteOperation(
      TEST_BLE_SERVICE_UUID, TEST_SECOND_WRITE_UUID, SECOND_WRITE_DATA
  )
  asserts.assert_true(
      write_operation_result, 'BLE write operation failed to start'
  )
  server_write_operation_result = server.server_callback.waitAndGet(
      'onCharacteristicWriteRequest', 30
  )
  asserts.assert_equal(
      server_write_operation_result.data['Data'], SECOND_WRITE_DATA
  )
  client.client_callback.waitAndGet('onCharacteristicWrite', 30)
  client.log.info('Second write operation finished')
