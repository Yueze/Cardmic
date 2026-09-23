/*
 * Cardmic development console (CARDMIC_DEV_TOOLS builds only).
 * See dev_console.cpp for the command set.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once

#ifdef CARDMIC_DEV_TOOLS

typedef void (*cardmic_dev_reply_t)(const char* line);

#ifdef __cplusplus
extern "C" {
#endif

// Starts the console on the USB serial/JTAG port (available in the launcher).
void cardmic_dev_console_start(void);

// Runs one command from another channel, e.g. the app's USB serial port.
void cardmic_dev_command(const char* line, cardmic_dev_reply_t reply_to);

#ifdef __cplusplus
}
#endif

#endif
