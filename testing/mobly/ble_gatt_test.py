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

"""BLE GATT connection test."""

import ble_utils
from mobly import base_test
from mobly import test_runner
from mobly.controllers import android_device


class BleBasicTest(base_test.BaseTestClass):
  """Tests the basic E2E connection flow for BLE."""

  def setup_class(self):
    self.ads = self.register_controller(android_device, min_number=2)
    for device in self.ads:
      device.load_snippet('mbs', android_device.MBS_PACKAGE)
      device.mbs.btEnable()
    self.initiator, self.receiver = self.ads[:2]
    # The initiator device scans BLE devices and behaves as a GATT client.
    self.initiator.debug_tag = 'initiator'
    # The receiver device advertises and behaves as a GATT server.
    self.receiver.debug_tag = 'receiver'

  def test_ble_gatt_read_write(self):
    """Test for making a GATT connection and reading and writing messages.

    Steps:
      1. Starts BLE scan and advertising, and complete the discovery process.
      2. Initiator connects to receiver.
      3. Initiator discovers the BLE service receiver provided.
      4. Initiator reads a message from receiver.
      5. Initiator sends a message to receiver.
      6. Initiator disconnects from receiver.
      7. BLE scan and advertising stopped.

    Verifies:
      In each step, initiator and receiver get corresponding callbacks.
    """
    ble_utils.Discover(self.initiator, self.receiver)
    ble_utils.Connect(self.initiator, self.receiver)
    ble_utils.DiscoverServices(self.initiator)
    ble_utils.ReadCharacteristic(self.initiator)
    ble_utils.WriteCharacteristic(self.initiator, self.receiver)
    ble_utils.Disconnect(self.initiator, self.receiver)
    ble_utils.StopDiscover(self.initiator, self.receiver)


if __name__ == '__main__':
  test_runner.main()
