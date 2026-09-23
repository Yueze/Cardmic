/*
 * Cardmic over-the-air updates from GitHub Releases. See cardmic_ota.h.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#include "cardmic_ota.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "esp_app_desc.h"
#include "esp_crt_bundle.h"
#include "esp_http_client.h"
#include "esp_https_ota.h"
#include "esp_log.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"

#define OTA_URL "https://github.com/" CARDMIC_OTA_REPO "/releases/latest/download/" CARDMIC_OTA_ASSET

static const char *TAG = "cardmic_ota";

static volatile cardmic_ota_state_t s_state = CARDMIC_OTA_IDLE;
static volatile int s_progress;
static char s_latest[32];
static char s_error[32];
static volatile int s_http_status;  // last HTTP status seen, after redirects

static void fail(const char *msg)
{
    strlcpy(s_error, msg, sizeof(s_error));
    s_state = CARDMIC_OTA_FAILED;
    ESP_LOGW(TAG, "%s", msg);
}

// "v0.5.1" / "0.6.0-beta.1" -> comparable integer. Missing parts count as 0.
// A pre-release ("-anything") ranks below the release of the same number, so
// a device on 0.6.0-beta.1 still updates to 0.6.0.
static long long version_key(const char *v)
{
    if (*v == 'v' || *v == 'V') v++;
    int a = 0, b = 0, c = 0;
    sscanf(v, "%d.%d.%d", &a, &b, &c);
    const bool pre = strchr(v, '-') != NULL;
    return ((long long)a * 1000000LL + b * 1000LL + c) * 2 + (pre ? 0 : 1);
}

static esp_err_t http_init(esp_http_client_handle_t client)
{
    return esp_http_client_set_header(client, "User-Agent", "cardmic");
}

static esp_err_t http_event(esp_http_client_event_t *evt)
{
    if (evt->event_id == HTTP_EVENT_ON_HEADER || evt->event_id == HTTP_EVENT_ON_FINISH) {
        s_http_status = esp_http_client_get_status_code(evt->client);
    }
    return ESP_OK;
}

// Opens the download and reads the image header. On ESP_OK the caller owns *out.
static esp_err_t open_latest(esp_https_ota_handle_t *out, esp_app_desc_t *desc)
{
    static esp_http_client_config_t http = {
        .url = OTA_URL,
        .crt_bundle_attach = esp_crt_bundle_attach,
        .timeout_ms = 15000,
        .buffer_size = 4096,     // GitHub's redirect responses carry large headers
        .buffer_size_tx = 2048,  // and the signed asset URL is ~1 KB long
        .max_redirection_count = 5,
        .keep_alive_enable = true,
        .event_handler = http_event,
    };
    s_http_status = 0;
    esp_https_ota_config_t ota = {.http_config = &http, .http_client_init_cb = http_init};
    esp_err_t err = esp_https_ota_begin(&ota, out);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "begin: %s", esp_err_to_name(err));
        return err;
    }
    err = esp_https_ota_get_img_desc(*out, desc);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "image header: %s", esp_err_to_name(err));
        esp_https_ota_abort(*out);
    }
    return err;
}

static void ota_task(void *arg)
{
    const bool install = (bool)(intptr_t)arg;
    esp_https_ota_handle_t h = NULL;
    esp_app_desc_t desc;

    if (open_latest(&h, &desc) != ESP_OK) {
        if (!install && s_http_status == 404) {
            // Nothing published (yet) that this device could install: the
            // running firmware is the newest there is.
            strlcpy(s_latest, esp_app_get_description()->version, sizeof(s_latest));
            s_state = CARDMIC_OTA_UP_TO_DATE;
        } else if (s_http_status == 0) {
            fail("NO CONNECTION TO UPDATE SERVER");
        } else {
            snprintf(s_error, sizeof(s_error), "CHECK FAILED (HTTP %d)", s_http_status);
            s_state = CARDMIC_OTA_FAILED;
        }
        vTaskDelete(NULL);
        return;
    }
    strlcpy(s_latest, desc.version, sizeof(s_latest));
    const bool newer = version_key(desc.version) > version_key(esp_app_get_description()->version);

    if (!install || !newer) {
        esp_https_ota_abort(h);
        s_state = newer ? CARDMIC_OTA_AVAILABLE : CARDMIC_OTA_UP_TO_DATE;
        vTaskDelete(NULL);
        return;
    }

    s_state = CARDMIC_OTA_DOWNLOADING;
    esp_err_t err;
    while ((err = esp_https_ota_perform(h)) == ESP_ERR_HTTPS_OTA_IN_PROGRESS) {
        int total = esp_https_ota_get_image_size(h);
        int done = esp_https_ota_get_image_len_read(h);
        if (total > 0) s_progress = done * 100 / total;
    }
    if (err != ESP_OK || !esp_https_ota_is_complete_data_received(h)) {
        esp_https_ota_abort(h);
        fail("DOWNLOAD INTERRUPTED");
        vTaskDelete(NULL);
        return;
    }
    if (esp_https_ota_finish(h) != ESP_OK) {
        fail("IMAGE CHECK FAILED");
        vTaskDelete(NULL);
        return;
    }
    s_progress = 100;
    s_state = CARDMIC_OTA_DONE;
    ESP_LOGI(TAG, "installed %s, restarting", s_latest);
    vTaskDelay(pdMS_TO_TICKS(1500));  // let the UI show it
    esp_restart();
}

static void start(bool install)
{
    s_error[0] = '\0';
    s_progress = 0;
    s_state = install ? CARDMIC_OTA_DOWNLOADING : CARDMIC_OTA_CHECKING;
    if (xTaskCreate(ota_task, "cardmic_ota", 8192, (void *)(intptr_t)install, 4, NULL) != pdPASS) {
        fail("OUT OF MEMORY");
    }
}

void cardmic_ota_check(void)
{
    if (s_state == CARDMIC_OTA_CHECKING || s_state == CARDMIC_OTA_DOWNLOADING || s_state == CARDMIC_OTA_DONE) return;
    start(false);
}

void cardmic_ota_install(void)
{
    if (s_state != CARDMIC_OTA_AVAILABLE) return;
    start(true);
}

void cardmic_ota_dismiss(void)
{
    if (s_state == CARDMIC_OTA_AVAILABLE || s_state == CARDMIC_OTA_UP_TO_DATE || s_state == CARDMIC_OTA_FAILED) {
        s_state = CARDMIC_OTA_IDLE;
    }
}

cardmic_ota_state_t cardmic_ota_state(void) { return s_state; }
const char *cardmic_ota_latest(void) { return s_latest; }
int cardmic_ota_progress(void) { return s_progress; }
const char *cardmic_ota_error(void) { return s_error; }
