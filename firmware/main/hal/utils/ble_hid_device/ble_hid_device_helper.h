/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
// https://github.com/espressif/esp-idf/blob/v5.4.2/examples/bluetooth/esp_hid_device
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    BLE_HID_DEVICE_STATE_IDLE = 0,
    BLE_HID_DEVICE_STATE_CONNECTED,
} BleHidDeviceState_t;

void ble_hid_device_helper_init(void);
void ble_hid_device_helper_send(uint8_t* buffer);
BleHidDeviceState_t ble_hid_device_helper_get_state(void);

#ifdef __cplusplus
}
#endif
