/*
 * Cardmic: USB Audio Class microphone (16 kHz, mono, 16-bit), driverless on
 * macOS and Windows.
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

#define CARDMIC_SAMPLE_RATE_HZ 16000
#define CARDMIC_SAMPLES_PER_MS (CARDMIC_SAMPLE_RATE_HZ / 1000)

// Install TinyUSB as a UAC microphone. Fails with ESP_ERR_INVALID_STATE if
// another USB function (the stock USB keyboard app) already owns TinyUSB.
esp_err_t cardmic_usb_start(void);

// Uninstall TinyUSB so other apps (USB keyboard) can use USB again.
void cardmic_usb_stop(void);

bool cardmic_usb_mounted(void);
bool cardmic_usb_streaming(void);
bool cardmic_usb_host_muted(void);

// Queue one frame of samples for the host. No-op unless the host is streaming.
void cardmic_usb_write(const int16_t *samples, size_t count);

#ifdef __cplusplus
}
#endif
