/*
 * Cardmic WiFi microphone: streams 16 kHz mono PCM over UDP to the Cardmic
 * desktop client. WiFi itself is brought up by the HAL (GetHAL().wifiConnect)
 * using the network saved in the stock "Set WiFi" app.
 *
 * Wire contract (must match the desktop client, see protocol docs):
 *  - listen on UDP 41234 for the 21-byte discovery "CPADV_MIC_DISCOVER_V1"
 *  - stream to the discovery packet's SOURCE address and port
 *  - drop the receiver 2.5 s after its last discovery (it is a keepalive)
 *  - 656-byte CPM1 packets: 16-byte header + 320 int16 samples (20 ms)
 *
 * Pairing: with pairing required, only "CPADV_MIC_DISCOVER_V2" + nonce[8] +
 * HMAC-SHA256(mac_key, first 29 bytes)[0..16] is accepted, and audio goes
 * out as 672-byte CPM2 packets encrypted with AES-128-GCM.
 *
 * Screenshots (for documentation): "CARDMIC_SCREENSHOT [page]" on the same
 * port is answered with the full 240x135 screen as CMSS packets, sent from an
 * ephemeral port to the requester. See protocol/PROTOCOL.md.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "esp_err.h"

#ifdef __cplusplus
extern "C" {
#endif

esp_err_t cardmic_net_start(void);
void cardmic_net_stop(void);

// Feed captured samples; packetised into 20 ms frames. Cheap no-op when no
// receiver is connected.
void cardmic_net_push(const int16_t *samples, size_t count);

bool cardmic_net_receiver_active(void);
// Receiver IPv4 as dotted string, or "" when none.
void cardmic_net_receiver_ip(char *out, size_t out_size);

// Current session is encrypted (pairing on, paired client connected).
bool cardmic_net_encrypted(void);

// Require pairing (keys from cardmic_pair_derive) or not. Ends any session.
void cardmic_net_set_pairing(bool required, const uint8_t enc_key[16], const uint8_t mac_key[16]);

// Milliseconds since an unpaired computer asked for audio and was refused
// (UINT32_MAX if never), for a hint on screen.
uint32_t cardmic_net_ms_since_unpaired(void);

// A screenshot request, once. addr/port are in network byte order; page is the
// optional page name that followed the request ("" for the current screen).
bool cardmic_net_take_screenshot_request(uint32_t *addr, uint16_t *port, char *page, size_t page_size);

#ifdef CARDMIC_DEV_TOOLS
// Development builds only: a key event sent as "CARDMIC_DEV_KEY <hex> <0|1> [name]".
bool cardmic_net_take_dev_key(uint8_t *code, bool *down, char *name, size_t name_size);
void cardmic_net_dev_set_code(const char *code);
#endif

#ifdef __cplusplus
}
#endif
