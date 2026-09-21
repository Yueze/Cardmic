/*
 * Cardmic over-the-air updates from GitHub Releases.
 *
 * The "cardmic-ota.bin" asset of the latest GitHub release is fetched through
 * github.com/<repo>/releases/latest/download/, which always redirects to the
 * newest release. The version is read from the image header itself (the first
 * few hundred bytes), so there is no API call and no JSON to parse. A newer
 * image is written to the idle OTA slot; if it fails to boot, the bootloader
 * rolls back to the previous one.
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CARDMIC_OTA_REPO "Yueze/cardmic"
#define CARDMIC_OTA_ASSET "cardmic-ota.bin"

typedef enum {
    CARDMIC_OTA_IDLE = 0,
    CARDMIC_OTA_CHECKING,
    CARDMIC_OTA_UP_TO_DATE,
    CARDMIC_OTA_AVAILABLE,
    CARDMIC_OTA_DOWNLOADING,
    CARDMIC_OTA_DONE,  // image written, about to restart
    CARDMIC_OTA_FAILED,
} cardmic_ota_state_t;

// Both run in a background task and return immediately. Needs Wi-Fi.
void cardmic_ota_check(void);
void cardmic_ota_install(void);

cardmic_ota_state_t cardmic_ota_state(void);
const char *cardmic_ota_latest(void);  // e.g. "0.5.1"; "" before a check
int cardmic_ota_progress(void);         // 0..100 while downloading
const char *cardmic_ota_error(void);    // short, upper-case, for the UI

#ifdef __cplusplus
}
#endif
