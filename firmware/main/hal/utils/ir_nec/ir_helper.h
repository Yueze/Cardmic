/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
// https://github.com/espressif/esp-idf/blob/v5.4.2/examples/peripherals/rmt/ir_nec_transceiver
#pragma once
#include <stdint.h>
#include <driver/gpio.h>

#ifdef __cplusplus
extern "C" {
#endif

void ir_helper_init(gpio_num_t pin_tx);
void ir_helper_send(uint8_t addr, uint8_t cmd);

#ifdef __cplusplus
}
#endif
