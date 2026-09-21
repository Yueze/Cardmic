/*
 * Cardmic WiFi microphone transport. See cardmic_net.h for the wire contract.
 *
 * SPDX-License-Identifier: MIT
 */
#include "cardmic_net.h"

#include <errno.h>
#include <fcntl.h>
#include <string.h>

#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "lwip/inet.h"
#include "lwip/sockets.h"

#define PORT 41234
#define FRAME_SAMPLES 320U
#define QUEUE_DEPTH 8U
#define RECEIVER_TIMEOUT_MS 2500U
#define SAMPLE_RATE 16000U

static const char *TAG = "cardmic_net";
static const char DISCOVERY[] = "CPADV_MIC_DISCOVER_V1";
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

static QueueHandle_t s_queue;
static TaskHandle_t s_task;
static volatile bool s_run;
static volatile bool s_receiver_active;
static volatile uint32_t s_receiver_addr;  // network byte order
static int16_t s_pending[FRAME_SAMPLES];
static size_t s_pending_count;

static volatile bool s_shot_wanted;
static struct sockaddr_in s_shot_to;
static char s_shot_page[16];

static void net_task(void *arg)
{
    (void)arg;
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
    uint32_t sequence = 0;
    packet_t pkt;
    memcpy(pkt.magic, "CPM1", 4);
    pkt.version = 1;
    pkt.flags = 0;
    pkt.sample_count = FRAME_SAMPLES;
    pkt.sample_rate = SAMPLE_RATE;

    while (s_run) {
        // Discovery doubles as the keepalive.
        uint8_t buf[48];
        struct sockaddr_in from;
        socklen_t from_len = sizeof(from);
        ssize_t n = recvfrom(sock, buf, sizeof(buf) - 1, 0, (struct sockaddr *)&from, &from_len);
        if (n >= (ssize_t)(sizeof(SCREENSHOT) - 1) && memcmp(buf, SCREENSHOT, sizeof(SCREENSHOT) - 1) == 0 &&
            !s_shot_wanted) {
            // "CARDMIC_SCREENSHOT" or "CARDMIC_SCREENSHOT <page>"; served by the UI task.
            buf[n] = '\0';
            const char *page = (const char *)buf + sizeof(SCREENSHOT) - 1;
            while (*page == ' ') page++;
            strlcpy(s_shot_page, page, sizeof(s_shot_page));
            s_shot_to = from;
            __sync_synchronize();
            s_shot_wanted = true;
        } else if (n == (ssize_t)(sizeof(DISCOVERY) - 1) && memcmp(buf, DISCOVERY, n) == 0) {
            bool same = s_receiver_active && from.sin_addr.s_addr == receiver.sin_addr.s_addr &&
                        from.sin_port == receiver.sin_port;
            // First-receiver lock: while a session is live, ignore discovery
            // from anyone else instead of handing them the stream.
            if (!s_receiver_active || same) {
                if (!same) {
                    sequence = 0;
                    xQueueReset(s_queue);
                }
                receiver = from;
                s_receiver_addr = from.sin_addr.s_addr;
                s_receiver_active = true;
                last_seen = xTaskGetTickCount();
            }
        }

        if (s_receiver_active && (xTaskGetTickCount() - last_seen) > pdMS_TO_TICKS(RECEIVER_TIMEOUT_MS)) {
            s_receiver_active = false;
            xQueueReset(s_queue);
        }

        frame_t frame;
        if (xQueueReceive(s_queue, &frame, pdMS_TO_TICKS(10)) == pdTRUE && s_receiver_active) {
            pkt.sequence = sequence++;
            memcpy(pkt.samples, frame.samples, sizeof(pkt.samples));
            sendto(sock, &pkt, sizeof(pkt), 0, (struct sockaddr *)&receiver, sizeof(receiver));
        }
    }
    close(sock);

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

bool cardmic_net_receiver_active(void) { return s_receiver_active; }

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
