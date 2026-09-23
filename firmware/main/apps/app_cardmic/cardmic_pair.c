/*
 * Cardmic pairing codes and key derivation. See cardmic_pair.h.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#include "cardmic_pair.h"

#include <string.h>

#include "esp_random.h"
#include "mbedtls/md.h"
#include "mbedtls/pkcs5.h"

// Crockford base32: no I, L, O or U, so a code survives being read aloud.
static const char ALPHABET[] = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
static const char SALT[] = "cardmic/pair/v1";
#define ITERATIONS 20000

void cardmic_pair_generate(char out[CARDMIC_PAIR_CODE_LEN + 1])
{
    uint8_t rnd[CARDMIC_PAIR_CODE_LEN];
    esp_fill_random(rnd, sizeof(rnd));
    for (int i = 0; i < CARDMIC_PAIR_CODE_LEN; ++i) {
        out[i] = ALPHABET[rnd[i] & 31];  // 32 symbols: unbiased
    }
    out[CARDMIC_PAIR_CODE_LEN] = '\0';
}

bool cardmic_pair_valid(const char *code)
{
    if (!code || strlen(code) != CARDMIC_PAIR_CODE_LEN) {
        return false;
    }
    for (int i = 0; i < CARDMIC_PAIR_CODE_LEN; ++i) {
        if (!strchr(ALPHABET, code[i]) || code[i] == '\0') {
            return false;
        }
    }
    return true;
}

void cardmic_pair_format(const char *code, char out[CARDMIC_PAIR_CODE_LEN + 3])
{
    int o = 0;
    for (int i = 0; i < CARDMIC_PAIR_CODE_LEN; ++i) {
        if (i && i % 4 == 0) out[o++] = '-';
        out[o++] = code[i];
    }
    out[o] = '\0';
}

bool cardmic_pair_derive(const char *code, uint8_t enc_key[16], uint8_t mac_key[16])
{
    if (!cardmic_pair_valid(code)) {
        return false;
    }
    uint8_t out[32];
    int err = mbedtls_pkcs5_pbkdf2_hmac_ext(MBEDTLS_MD_SHA256, (const unsigned char *)code, CARDMIC_PAIR_CODE_LEN,
                                            (const unsigned char *)SALT, sizeof(SALT) - 1, ITERATIONS, sizeof(out), out);
    if (err != 0) {
        return false;
    }
    memcpy(enc_key, out, 16);
    memcpy(mac_key, out + 16, 16);
    memset(out, 0, sizeof(out));
    return true;
}
