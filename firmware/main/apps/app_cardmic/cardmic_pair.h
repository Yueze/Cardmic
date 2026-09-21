/*
 * Cardmic pairing: a 12-character code shown on the device, typed once into
 * the desktop client ("cardmic pair XXXX-XXXX-XXXX"). Both sides derive the
 * same keys from it; with pairing on, discovery must carry an HMAC made with
 * those keys and the audio is encrypted with AES-128-GCM. See PROTOCOL.md.
 *
 * SPDX-License-Identifier: MIT
 */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define CARDMIC_PAIR_CODE_LEN 12  // Crockford base32, 60 bits

// A fresh random code (NUL-terminated, no dashes) from the hardware RNG.
void cardmic_pair_generate(char out[CARDMIC_PAIR_CODE_LEN + 1]);

// True if `code` is 12 valid Crockford base32 characters (upper case, no dashes).
bool cardmic_pair_valid(const char *code);

// "ABCD-EFGH-JKMN"
void cardmic_pair_format(const char *code, char out[CARDMIC_PAIR_CODE_LEN + 3]);

// PBKDF2-HMAC-SHA256(code, "cardmic/pair/v1", 20000) -> 32 bytes:
// the first 16 are the AES-128-GCM audio key, the last 16 the HMAC key.
// Takes a noticeable fraction of a second; call off the UI loop.
bool cardmic_pair_derive(const char *code, uint8_t enc_key[16], uint8_t mac_key[16]);

#ifdef __cplusplus
}
#endif
