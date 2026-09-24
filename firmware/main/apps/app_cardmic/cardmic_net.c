/*
 * Cardmic WiFi microphone transport. See cardmic_net.h for the wire contract.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#include "cardmic_net.h"

#include <errno.h>
#include <fcntl.h>
#include <string.h>

#include "esp_log.h"
#include "esp_netif.h"
#include "esp_random.h"
#include "mbedtls/gcm.h"
#include "mbedtls/md.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "lwip/inet.h"
#include "lwip/sockets.h"
#ifdef CARDMIC_DEV_TOOLS
#include <stdio.h>
#include "esp_system.h"
#include "soc/rtc_cntl_reg.h"
#endif

#define PORT 41234
#define FRAME_SAMPLES 320U
#define QUEUE_DEPTH 8U
#define RECEIVER_TIMEOUT_MS 2500U
#define SAMPLE_RATE 16000U

static const char *TAG = "cardmic_net";
static const char DISCOVERY[] = "CPADV_MIC_DISCOVER_V1";
static const char DISCOVERY_V2[] = "CPADV_MIC_DISCOVER_V2";  // + nonce[8] + hmac[16]
#define DISCOVERY_V2_LEN (21 + 8 + 16)
static const char SCREENSHOT[] = "CARDMIC_SCREENSHOT";

typedef struct {
    int16_t samples[FRAME_SAMPLES];
} frame_t;

typedef struct __attribute__((packed)) {
    char magic[4];
    uint8_t version;
    uint8_t flags;
    uint16_t sample_count;
    uint32_t sequence;
    uint32_t sample_rate;
    int16_t samples[FRAME_SAMPLES];
} packet_t;

// Encrypted audio. Same 16-byte header, but the last word carries a random
// per-session id instead of the (fixed) sample rate, followed by the GCM tag.
typedef struct __attribute__((packed)) {
    char magic[4];
    uint8_t version;
    uint8_t flags;
    uint16_t sample_count;
    uint32_t sequence;
    uint32_t session;
    uint8_t tag[16];
    uint8_t ciphertext[FRAME_SAMPLES * 2];
} packet2_t;

// Pairing state, set from the UI task and picked up by the net task.
static portMUX_TYPE s_pair_lock = portMUX_INITIALIZER_UNLOCKED;
static struct {
    bool required;
    uint8_t enc_key[16];
    uint8_t mac_key[16];
    uint32_t generation;
} s_pair;
static volatile uint32_t s_unpaired_at;  // tick of the last discovery refused for lack of pairing

// The Cardputer's name, told to the computer it streams to (see PROTOCOL.md).
static const char NAME_PREFIX[] = "CARDMIC_NAME ";
#define NAME_EVERY_MS 2000
static char s_name[40] = "Cardmic";
static volatile bool s_session_encrypted;

static QueueHandle_t s_queue;
static TaskHandle_t s_task;
static volatile bool s_run;
static volatile bool s_receiver_active;
static volatile uint32_t s_receiver_addr;  // network byte order
static int16_t s_pending[FRAME_SAMPLES];
static size_t s_pending_count;

#ifdef CARDMIC_DEV_TOOLS
// Development-only remote control, compiled out of release builds (CI does
// not define CARDMIC_DEV_TOOLS). Lets a developer reflash and drive the UI
// without pressing buttons:
//   CARDMIC_DEV_DOWNLOAD              reboot into the ROM serial bootloader
//   CARDMIC_DEV_RESTART               plain reboot
//   CARDMIC_DEV_KEY <hex> <0|1> [nm]  key down/up, e.g. "2c 1" = space down
static QueueHandle_t s_dev_keys;
typedef struct {
    uint8_t code;
    bool down;
    char name[4];
} dev_key_t;

static char s_dev_code[16];
void cardmic_net_dev_set_code(const char *code) { strlcpy(s_dev_code, code, sizeof(s_dev_code)); }

static void dev_command(const char *cmd, int sock, const struct sockaddr_in *from)
{
    if (strcmp(cmd, "CARDMIC_DEV_CODE") == 0) {  // read the pairing code back
        sendto(sock, s_dev_code, strlen(s_dev_code), 0, (const struct sockaddr *)from, sizeof(*from));
        return;
    }
    // Never tear USB down here: this task runs on USB (the app's serial
    // console) or beside it, and uninstalling TinyUSB from it deadlocks both
    // channels. The reset reinitialises USB anyway.
    if (strcmp(cmd, "CARDMIC_DEV_DOWNLOAD") == 0) {
        ESP_LOGW(TAG, "dev: rebooting into download mode");
        REG_WRITE(RTC_CNTL_OPTION1_REG, RTC_CNTL_FORCE_DOWNLOAD_BOOT);
        esp_restart();
    } else if (strcmp(cmd, "CARDMIC_DEV_RESTART") == 0) {
        esp_restart();
    } else if (strncmp(cmd, "CARDMIC_DEV_KEY ", 16) == 0 && s_dev_keys) {
        unsigned code = 0, down = 0;
        dev_key_t k = {0};
        char name[4] = "";
        if (sscanf(cmd + 16, "%x %u %3s", &code, &down, name) >= 2) {
            k.code = (uint8_t)code;
            k.down = down != 0;
            strlcpy(k.name, name, sizeof(k.name));
            xQueueSend(s_dev_keys, &k, 0);
        }
    }
}

bool cardmic_net_take_dev_key(uint8_t *code, bool *down, char *name, size_t name_size)
{
    dev_key_t k;
    if (!s_dev_keys || xQueueReceive(s_dev_keys, &k, 0) != pdTRUE) {
        return false;
    }
    *code = k.code;
    *down = k.down;
    strlcpy(name, k.name, name_size);
    return true;
}
#endif

static volatile bool s_shot_wanted;
static struct sockaddr_in s_shot_to;
static char s_shot_page[16];

// Constant-time comparison for authentication tags.
static bool tags_equal(const uint8_t *a, const uint8_t *b, size_t n)
{
    uint8_t d = 0;
    for (size_t i = 0; i < n; ++i) d |= a[i] ^ b[i];
    return d == 0;
}

// Challenge and response (PROTOCOL.md, section 3). A v2 discovery can be
// recorded and sent again by anyone on the network, so a sender proves it is
// live by answering a random challenge. One that has not answered (a
// recording, or an app from before 0.7.0) holds the stream only until one that
// has comes along, and is not told the device's name.
static const char CHALLENGE[] = "CARDMIC_CHALLENGE";  // + challenge[16]
static const char RESPONSE[] = "CPADV_MIC_RESPONSE";  // + challenge[16] + hmac[16]
#define RESPONSE_PREFIX_LEN (sizeof(RESPONSE) - 1)
#define CHALLENGE_BYTES 16
#define RESPONSE_LEN (RESPONSE_PREFIX_LEN + CHALLENGE_BYTES + 16)
#define CHALLENGE_LIFE_MS 5000U
#define CHALLENGE_EVERY_MS 500U
#define MAX_CHALLENGES 4
// A receiver that answered is asked again every REVERIFY_MS, and counts as
// not answered after VERIFIED_FOR_MS without an answer, so a computer that
// left cannot be kept "answered" by someone repeating its discovery.
#define REVERIFY_MS 20000U
#define VERIFIED_FOR_MS 30000U

typedef struct {
    bool used;
    struct sockaddr_in to;
    uint8_t bytes[CHALLENGE_BYTES];
    TickType_t issued;
    TickType_t sent;
} challenge_t;

static bool same_addr(const struct sockaddr_in *a, const struct sockaddr_in *b)
{
    return a->sin_addr.s_addr == b->sin_addr.s_addr && a->sin_port == b->sin_port;
}

// Send `to` its challenge: a new one, or again the one it has, at most every
// CHALLENGE_EVERY_MS. The table holds the last few senders' challenges.
static void challenge(int sock, challenge_t *table, const struct sockaddr_in *to)
{
    const TickType_t now = xTaskGetTickCount();
    challenge_t *slot = NULL;
    for (int i = 0; i < MAX_CHALLENGES; ++i) {
        if (table[i].used && now - table[i].issued > pdMS_TO_TICKS(CHALLENGE_LIFE_MS)) table[i].used = false;
        if (table[i].used && same_addr(&table[i].to, to)) slot = &table[i];
    }
    if (slot) {
        if (now - slot->sent < pdMS_TO_TICKS(CHALLENGE_EVERY_MS)) return;
    } else {
        for (int i = 0; i < MAX_CHALLENGES && !slot; ++i) {
            if (!table[i].used) slot = &table[i];
        }
        if (!slot) {  // all taken: the oldest goes
            slot = &table[0];
            for (int i = 1; i < MAX_CHALLENGES; ++i) {
                if (now - table[i].issued > now - slot->issued) slot = &table[i];
            }
        }
        slot->used = true;
        slot->to = *to;
        esp_fill_random(slot->bytes, CHALLENGE_BYTES);
        slot->issued = now;
    }
    slot->sent = now;
    uint8_t msg[sizeof(CHALLENGE) - 1 + CHALLENGE_BYTES];
    memcpy(msg, CHALLENGE, sizeof(CHALLENGE) - 1);
    memcpy(msg + sizeof(CHALLENGE) - 1, slot->bytes, CHALLENGE_BYTES);
    sendto(sock, msg, sizeof(msg), 0, (const struct sockaddr *)to, sizeof(*to));
}

// Is this the answer to the challenge sent to `from`? The MAC covers this
// device's own address, so an answer a paired computer gave to a challenge
// relayed to it by someone else does not pass. Each challenge answers once.
static bool answered(const uint8_t *buf, ssize_t n, const struct sockaddr_in *from, challenge_t *table,
                     const uint8_t *mac_key)
{
    if (n != (ssize_t)RESPONSE_LEN || memcmp(buf, RESPONSE, RESPONSE_PREFIX_LEN) != 0) {
        return false;
    }
    esp_netif_t *sta = esp_netif_get_handle_from_ifkey("WIFI_STA_DEF");
    esp_netif_ip_info_t ip;
    if (!sta || esp_netif_get_ip_info(sta, &ip) != ESP_OK) {
        return false;
    }
    const TickType_t now = xTaskGetTickCount();
    for (int i = 0; i < MAX_CHALLENGES; ++i) {
        challenge_t *c = &table[i];
        if (!c->used || !same_addr(&c->to, from) || now - c->issued > pdMS_TO_TICKS(CHALLENGE_LIFE_MS) ||
            memcmp(c->bytes, buf + RESPONSE_PREFIX_LEN, CHALLENGE_BYTES) != 0) {
            continue;
        }
        uint8_t msg[RESPONSE_PREFIX_LEN + CHALLENGE_BYTES + 6];
        memcpy(msg, buf, RESPONSE_PREFIX_LEN + CHALLENGE_BYTES);
        memcpy(msg + RESPONSE_PREFIX_LEN + CHALLENGE_BYTES, &ip.ip.addr, 4);  // network byte order
        msg[sizeof(msg) - 2] = PORT >> 8;
        msg[sizeof(msg) - 1] = PORT & 0xFF;
        uint8_t mac[32];
        const mbedtls_md_info_t *sha256 = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
        if (mbedtls_md_hmac(sha256, mac_key, 16, msg, sizeof(msg), mac) == 0 &&
            tags_equal(mac, buf + RESPONSE_PREFIX_LEN + CHALLENGE_BYTES, 16)) {
            c->used = false;
            return true;
        }
    }
    return false;
}

typedef enum { DISCOVERY_NONE, DISCOVERY_ACCEPTED, DISCOVERY_REFUSED } discovery_t;

// Is this a discovery packet, and may its sender receive audio? Sets
// *encrypted for sessions that must be encrypted. REFUSED: a discovery
// without the current pairing code (never paired, or the code changed).
static discovery_t accept_discovery(const uint8_t *buf, ssize_t n, bool required, const uint8_t *mac_key, bool *encrypted)
{
    const bool v1 = n == (ssize_t)(sizeof(DISCOVERY) - 1) && memcmp(buf, DISCOVERY, n) == 0;
    const bool v2 = n == DISCOVERY_V2_LEN && memcmp(buf, DISCOVERY_V2, sizeof(DISCOVERY_V2) - 1) == 0;
    if (!v1 && !v2) {
        return DISCOVERY_NONE;
    }
    if (!required) {
        *encrypted = false;  // pairing off: anyone on the network, unencrypted
        return DISCOVERY_ACCEPTED;
    }
    if (v2) {
        uint8_t mac[32];
        const mbedtls_md_info_t *sha256 = mbedtls_md_info_from_type(MBEDTLS_MD_SHA256);
        if (mbedtls_md_hmac(sha256, mac_key, 16, buf, 29, mac) == 0 && tags_equal(mac, buf + 29, 16)) {
            *encrypted = true;
            return DISCOVERY_ACCEPTED;
        }
    }
    s_unpaired_at = xTaskGetTickCount();
    return DISCOVERY_REFUSED;
}

static void net_task(void *arg)
{
    (void)arg;
    mbedtls_gcm_context gcm;
    mbedtls_gcm_init(&gcm);
    uint32_t pair_generation = 0;
    bool pair_required = false;
    uint8_t mac_key[16] = {0};
    int sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_IP);
    if (sock < 0) {
        ESP_LOGE(TAG, "socket: %d", errno);
        goto done;
    }
    struct sockaddr_in bind_addr = {
        .sin_family = AF_INET,
        .sin_port = htons(PORT),
        .sin_addr.s_addr = htonl(INADDR_ANY),
    };
    if (bind(sock, (struct sockaddr *)&bind_addr, sizeof(bind_addr)) != 0) {
        ESP_LOGE(TAG, "bind: %d", errno);
        close(sock);
        goto done;
    }
    fcntl(sock, F_SETFL, fcntl(sock, F_GETFL, 0) | O_NONBLOCK);

    struct sockaddr_in receiver = {0};
    TickType_t last_seen = 0;
    TickType_t name_told = 0;
    bool name_due = false;
    bool verified = false;  // the receiver answered a challenge (pairing on)
    TickType_t verified_at = 0;
    challenge_t challenges[MAX_CHALLENGES] = {0};
    uint32_t sequence = 0;
    packet_t pkt;
    memcpy(pkt.magic, "CPM1", 4);
    pkt.version = 1;
    pkt.flags = 0;
    pkt.sample_count = FRAME_SAMPLES;
    pkt.sample_rate = SAMPLE_RATE;
    packet2_t pkt2;
    memcpy(pkt2.magic, "CPM2", 4);
    pkt2.version = 2;
    pkt2.flags = 0;
    pkt2.sample_count = FRAME_SAMPLES;
    bool encrypted = false;

    while (s_run) {
        // Pick up a pairing change; it ends any session in progress.
        if (s_pair.generation != pair_generation) {
            uint8_t enc_key[16];
            taskENTER_CRITICAL(&s_pair_lock);
            pair_generation = s_pair.generation;
            pair_required = s_pair.required;
            memcpy(enc_key, s_pair.enc_key, 16);
            memcpy(mac_key, s_pair.mac_key, 16);
            taskEXIT_CRITICAL(&s_pair_lock);
            mbedtls_gcm_free(&gcm);
            mbedtls_gcm_init(&gcm);
            if (pair_required) mbedtls_gcm_setkey(&gcm, MBEDTLS_CIPHER_ID_AES, enc_key, 128);
            memset(enc_key, 0, sizeof(enc_key));
            s_receiver_active = false;
            verified = false;
            memset(challenges, 0, sizeof(challenges));
        }

        // Discovery doubles as the keepalive.
        uint8_t buf[64];
        struct sockaddr_in from;
        socklen_t from_len = sizeof(from);
        ssize_t n = recvfrom(sock, buf, sizeof(buf) - 1, 0, (struct sockaddr *)&from, &from_len);
#ifdef CARDMIC_DEV_TOOLS
        if (n > 12 && memcmp(buf, "CARDMIC_DEV_", 12) == 0) {
            buf[n] = '\0';
            dev_command((const char *)buf, sock, &from);
            continue;
        }
#endif
        bool session_encrypted = false;
        discovery_t discovery = DISCOVERY_NONE;
        // Screenshots show whatever is on screen; not while pairing is on. Say
        // so rather than staying silent, which reads as "device offline".
        // Development builds take them anyway, for the docs.
#ifdef CARDMIC_DEV_TOOLS
        const bool shots_refused = false;
#else
        const bool shots_refused = pair_required;
#endif
        if (shots_refused && n >= (ssize_t)(sizeof(SCREENSHOT) - 1) &&
            memcmp(buf, SCREENSHOT, sizeof(SCREENSHOT) - 1) == 0) {
            static const char denied[] = "CARDMIC_PAIRING_REQUIRED";
            sendto(sock, denied, sizeof(denied) - 1, 0, (struct sockaddr *)&from, sizeof(from));
        } else if (!shots_refused && n >= (ssize_t)(sizeof(SCREENSHOT) - 1) &&
            memcmp(buf, SCREENSHOT, sizeof(SCREENSHOT) - 1) == 0 && !s_shot_wanted) {
            // "CARDMIC_SCREENSHOT" or "CARDMIC_SCREENSHOT <page>"; served by the UI task.
            buf[n] = '\0';
            const char *page = (const char *)buf + sizeof(SCREENSHOT) - 1;
            while (*page == ' ') page++;
            strlcpy(s_shot_page, page, sizeof(s_shot_page));
            s_shot_to = from;
            __sync_synchronize();
            s_shot_wanted = true;
        } else if (n > 0 && (discovery = accept_discovery(buf, n, pair_required, mac_key, &session_encrypted)) ==
                                DISCOVERY_REFUSED) {
            // Tell the computer why nothing comes, at most once a second, so it
            // can ask for the (new) code instead of searching forever. It
            // learns only that pairing is on, which the screen shows anyway.
            static TickType_t last_told;
            if (xTaskGetTickCount() - last_told > pdMS_TO_TICKS(1000)) {
                last_told = xTaskGetTickCount();
                static const char denied[] = "CARDMIC_PAIRING_REQUIRED";
                sendto(sock, denied, sizeof(denied) - 1, 0, (struct sockaddr *)&from, sizeof(from));
            }
        } else if (discovery == DISCOVERY_ACCEPTED) {
            bool same = s_receiver_active && same_addr(&from, &receiver);
            // First-receiver lock: while a session is live, discovery from
            // anyone else does not hand them the stream.
            if (!s_receiver_active || same) {
                if (!same) {
                    sequence = 0;
                    // A fresh random session id per receiver keeps GCM nonces
                    // (session, sequence) unique although sequences restart.
                    pkt2.session = esp_random();
                    encrypted = session_encrypted;
                    s_session_encrypted = encrypted;
                    xQueueReset(s_queue);
                    verified = !pair_required;
                    name_due = true;
                }
                receiver = from;
                s_receiver_addr = from.sin_addr.s_addr;
                s_receiver_active = true;
                last_seen = xTaskGetTickCount();
            }
            // With pairing on, every sender but a receiver that has answered is
            // asked to prove it is live, and that one again now and then;
            // 0.7.0 apps answer.
            const bool answered_receiver = s_receiver_active && same_addr(&from, &receiver) && verified;
            if (pair_required && (!answered_receiver || xTaskGetTickCount() - verified_at > pdMS_TO_TICKS(REVERIFY_MS))) {
                challenge(sock, challenges, &from);
            }
        } else if (pair_required && n > 0 && answered(buf, n, &from, challenges, mac_key)) {
            // A live paired computer. It takes the stream from a receiver that
            // has not answered, but not from another that has.
            bool same = s_receiver_active && same_addr(&from, &receiver);
            if (!s_receiver_active || same || !verified) {
                if (!same) {
                    sequence = 0;
                    pkt2.session = esp_random();
                    encrypted = true;
                    s_session_encrypted = true;
                    xQueueReset(s_queue);
                }
                receiver = from;
                s_receiver_addr = from.sin_addr.s_addr;
                s_receiver_active = true;
                last_seen = xTaskGetTickCount();
                verified = true;
                verified_at = last_seen;
                name_due = true;
            }
        }

        // Say who this is to a receiver that may know (with pairing on, one
        // that answered a challenge): on a new session, then now and then, so
        // a renamed Cardputer shows its new name without reconnecting.
        if (s_receiver_active && verified &&
            (name_due || xTaskGetTickCount() - name_told > pdMS_TO_TICKS(NAME_EVERY_MS))) {
            char name[sizeof(s_name)];
            taskENTER_CRITICAL(&s_pair_lock);
            memcpy(name, s_name, sizeof(name));
            taskEXIT_CRITICAL(&s_pair_lock);
            char msg[sizeof(NAME_PREFIX) + sizeof(name)];
            int len = snprintf(msg, sizeof(msg), "%s%s", NAME_PREFIX, name);
            sendto(sock, msg, len, 0, (struct sockaddr *)&receiver, sizeof(receiver));
            name_told = xTaskGetTickCount();
            name_due = false;
        }

        if (verified && pair_required && xTaskGetTickCount() - verified_at > pdMS_TO_TICKS(VERIFIED_FOR_MS)) {
            verified = false;  // takes over like any sender that has not answered
        }

        if (s_receiver_active && (xTaskGetTickCount() - last_seen) > pdMS_TO_TICKS(RECEIVER_TIMEOUT_MS)) {
            s_receiver_active = false;
            verified = false;
            xQueueReset(s_queue);
        }

        frame_t frame;
        if (xQueueReceive(s_queue, &frame, pdMS_TO_TICKS(10)) == pdTRUE && s_receiver_active) {
            if (encrypted) {
                pkt2.sequence = sequence++;
                uint8_t iv[12] = {0};
                memcpy(iv, &pkt2.session, 4);   // little-endian, as on the wire
                memcpy(iv + 4, &pkt2.sequence, 4);
                if (mbedtls_gcm_crypt_and_tag(&gcm, MBEDTLS_GCM_ENCRYPT, sizeof(pkt2.ciphertext), iv, sizeof(iv),
                                              (const unsigned char *)&pkt2, 16, (const unsigned char *)frame.samples,
                                              pkt2.ciphertext, sizeof(pkt2.tag), pkt2.tag) == 0) {
                    sendto(sock, &pkt2, sizeof(pkt2), 0, (struct sockaddr *)&receiver, sizeof(receiver));
                }
            } else {
                pkt.sequence = sequence++;
                memcpy(pkt.samples, frame.samples, sizeof(pkt.samples));
                sendto(sock, &pkt, sizeof(pkt), 0, (struct sockaddr *)&receiver, sizeof(receiver));
            }
        }
    }
    close(sock);
    mbedtls_gcm_free(&gcm);

done:
    s_receiver_active = false;
    s_task = NULL;
    vTaskDelete(NULL);
}

esp_err_t cardmic_net_start(void)
{
    if (s_task) {
        return ESP_OK;
    }
    if (!s_queue) {
        s_queue = xQueueCreate(QUEUE_DEPTH, sizeof(frame_t));
        if (!s_queue) {
            return ESP_ERR_NO_MEM;
        }
    }
    xQueueReset(s_queue);
    s_pending_count = 0;
#ifdef CARDMIC_DEV_TOOLS
    if (!s_dev_keys) s_dev_keys = xQueueCreate(16, sizeof(dev_key_t));
#endif
    s_run = true;
    if (xTaskCreate(net_task, "cardmic_net", 6144, NULL, 5, &s_task) != pdPASS) {
        s_run = false;
        return ESP_ERR_NO_MEM;
    }
    return ESP_OK;
}

void cardmic_net_stop(void)
{
    s_run = false;
    for (int i = 0; i < 50 && s_task; ++i) {
        vTaskDelay(pdMS_TO_TICKS(10));
    }
    s_receiver_active = false;
}

void cardmic_net_push(const int16_t *samples, size_t count)
{
    if (!s_queue || !s_receiver_active) {
        s_pending_count = 0;
        return;
    }
    while (count > 0) {
        size_t take = FRAME_SAMPLES - s_pending_count;
        if (take > count) {
            take = count;
        }
        memcpy(&s_pending[s_pending_count], samples, take * sizeof(int16_t));
        s_pending_count += take;
        samples += take;
        count -= take;
        if (s_pending_count == FRAME_SAMPLES) {
            (void)xQueueSend(s_queue, s_pending, 0);  // drop if the link is congested
            s_pending_count = 0;
        }
    }
}

void cardmic_net_set_name(const char *name)
{
    taskENTER_CRITICAL(&s_pair_lock);
    strlcpy(s_name, name, sizeof(s_name));
    taskEXIT_CRITICAL(&s_pair_lock);
}

bool cardmic_net_receiver_active(void) { return s_receiver_active; }
bool cardmic_net_encrypted(void) { return s_receiver_active && s_session_encrypted; }

void cardmic_net_set_pairing(bool required, const uint8_t enc_key[16], const uint8_t mac_key[16])
{
    taskENTER_CRITICAL(&s_pair_lock);
    s_pair.required = required;
    if (enc_key) memcpy(s_pair.enc_key, enc_key, 16);
    if (mac_key) memcpy(s_pair.mac_key, mac_key, 16);
    s_pair.generation++;
    taskEXIT_CRITICAL(&s_pair_lock);
}

uint32_t cardmic_net_ms_since_unpaired(void)
{
    uint32_t at = s_unpaired_at;
    if (!at) return UINT32_MAX;
    return (xTaskGetTickCount() - at) * portTICK_PERIOD_MS;
}

bool cardmic_net_take_screenshot_request(uint32_t *addr, uint16_t *port, char *page, size_t page_size)
{
    if (!s_shot_wanted) {
        return false;
    }
    *addr = s_shot_to.sin_addr.s_addr;
    *port = s_shot_to.sin_port;
    strlcpy(page, s_shot_page, page_size);
    s_shot_wanted = false;
    return true;
}

void cardmic_net_receiver_ip(char *out, size_t out_size)
{
    if (!s_receiver_active) {
        out[0] = '\0';
        return;
    }
    struct in_addr a = {.s_addr = s_receiver_addr};
    strlcpy(out, inet_ntoa(a), out_size);
}
