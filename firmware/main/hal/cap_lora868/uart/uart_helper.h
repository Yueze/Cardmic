/*
 * SPDX-FileCopyrightText: 2025 M5Stack Technology CO LTD
 *
 * SPDX-License-Identifier: MIT
 */
// https://github.com/espressif/esp-idf/blob/v5.4.2/examples/peripherals/uart/uart_echo/main/uart_echo_example_main.c
#pragma once
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*gps_uart_helper_on_msg_callback_t)(const char* msg);

void gps_uart_helper_init();
void gps_uart_helper_set_on_msg_callback(gps_uart_helper_on_msg_callback_t callback);

#ifdef __cplusplus
}
#endif
