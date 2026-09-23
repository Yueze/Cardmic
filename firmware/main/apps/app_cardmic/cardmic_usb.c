/*
 * Cardmic USB Audio Class microphone. Ported from the standalone Cardmic
 * firmware, which was verified on hardware (enumerates driverless on macOS,
 * picked up automatically by Wispr Flow).
 *
 * The CFG_TUD_AUDIO* options this relies on are set as GLOBAL compile options
 * in the project CMakeLists.txt: the TinyUSB audio class driver lives in the
 * espressif/tinyusb component and is compiled out unless its own sources see
 * CFG_TUD_AUDIO=1.
 *
 * SPDX-License-Identifier: Apache-2.0
 */
#include "cardmic_usb.h"

#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_mac.h"
#include "esp_private/usb_phy.h"
#include "esp_system.h"
#include "hal/usb_serial_jtag_ll.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#ifdef CARDMIC_DEV_TOOLS
#include "../dev_console.h"
#include "freertos/queue.h"
#include "tusb_cdc_acm.h"
#include "tusb_console.h"
#endif
#include "class/hid/hid_device.h"
#include "tinyusb.h"
#include "tusb.h"

#define AUDIO_BYTES_PER_SAMPLE CFG_TUD_AUDIO_FUNC_1_N_BYTES_PER_SAMPLE_TX
#define AUDIO_CHANNEL_COUNT CFG_TUD_AUDIO_FUNC_1_N_CHANNELS_TX
#define USB_AUDIO_EP_IN 0x81
#define USB_HID_EP_IN 0x82
// Every configuration is the microphone plus one HID interface: an identity
// report the Cardmic app reads to pair over USB, and, with the talk key on,
// the keyboard too (same interface, so no extra endpoint).
#define USB_CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + TUD_AUDIO_MIC_ONE_CH_DESC_LEN + TUD_HID_DESC_LEN)
// Mic-only and mic+keyboard are different USB products: hosts (Windows in
// particular) cache a device's interfaces per VID/PID. 0x4011/0x4012 were the
// same without the identity report.
#define USB_PID_MIC 0x4015
#define USB_PID_MIC_KEYBOARD 0x4016
#ifdef CARDMIC_DEV_TOOLS
// Development builds carry a USB serial port as well, so they are different
// USB products again (hosts cache a device's interfaces per VID/PID).
#define USB_PID_DEV_MIC 0x4017
#define USB_PID_DEV_MIC_KEYBOARD 0x4018
#define USB_CDC_EP_NOTIF 0x83
#define USB_CDC_EP_OUT 0x03
#define USB_CDC_EP_IN 0x84
#endif

static const char *TAG = "cardmic_usb";

enum {
    ITF_NUM_AUDIO_CONTROL = 0,
    ITF_NUM_AUDIO_STREAMING,
    ITF_NUM_HID,
    ITF_NUM_TOTAL,
};

enum {
    STRID_LANGID = 0,
    STRID_MANUFACTURER,
    STRID_PRODUCT,
    STRID_SERIAL,
    STRID_AUDIO_INTERFACE,
    STRID_HID_INTERFACE,
    STRID_CDC_INTERFACE,
    STRID_ID_INTERFACE,
};

static volatile bool s_installed;
static volatile bool s_mounted;
static volatile bool s_streaming;
static bool s_mute[AUDIO_CHANNEL_COUNT + 1];
static int16_t s_volume[AUDIO_CHANNEL_COUNT + 1];
static uint32_t s_sample_freq = CARDMIC_SAMPLE_RATE_HZ;
static uint8_t s_clock_valid = 1;
static audio_control_range_2_n_t(1) s_volume_range;
static audio_control_range_4_n_t(1) s_sample_freq_range;
static char s_usb_serial[13] = "000000000000";
static bool s_keyboard;  // this session enumerates the talk-key keyboard too
// Set while the USB PHY is handed back to the serial/JTAG controller. It must
// be deleted before TinyUSB can take the PHY again, or a restart (switching
// the talk key on or off) leaves the device with no USB at all.
static usb_phy_handle_t s_jtag_phy;

static tusb_desc_device_t s_device_descriptor = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = TUSB_CLASS_MISC,
    .bDeviceSubClass = MISC_SUBCLASS_COMMON,
    .bDeviceProtocol = MISC_PROTOCOL_IAD,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,
    // TinyUSB placeholder VID. Replace with an Espressif-allocated PID
    // (VID 0x303A, espressif/usb-pids) before public release.
    .idVendor = 0xCafe,
    .idProduct = USB_PID_MIC,
    .bcdDevice = 0x0300,
    .iManufacturer = STRID_MANUFACTURER,
    .iProduct = STRID_PRODUCT,
    .iSerialNumber = STRID_SERIAL,
    .bNumConfigurations = 1,
};

// Identity: vendor-defined feature report 3, 63 bytes of ASCII, e.g.
// "CM1;pair=1;code=7K2M9QXB4TPA;name=Cardmic-05AC;fw=0.6.0". The code is
// there only while pairing is on: plugging in is what pairs a computer.
#define CARDMIC_ID_REPORT 3
#define CARDMIC_ID_LEN 63
#define TUD_HID_REPORT_DESC_CARDMIC_ID                                                           \
    HID_USAGE_PAGE_N(0xFF00, 2), HID_USAGE(0x01), HID_COLLECTION(HID_COLLECTION_APPLICATION),  \
        HID_REPORT_ID(CARDMIC_ID_REPORT) HID_USAGE(0x02), HID_LOGICAL_MIN(0x00),                \
        HID_LOGICAL_MAX_N(0x00FF, 2), HID_REPORT_SIZE(8), HID_REPORT_COUNT(CARDMIC_ID_LEN),      \
        HID_FEATURE(HID_DATA | HID_VARIABLE | HID_ABSOLUTE), HID_COLLECTION_END

static const uint8_t s_hid_id_only[] = {TUD_HID_REPORT_DESC_CARDMIC_ID};
// With the talk key: the stock keyboard (report 1) and mouse (report 2), then identity.
static const uint8_t s_hid_kb_and_id[] = {
    TUD_HID_REPORT_DESC_KEYBOARD(HID_REPORT_ID(HID_ITF_PROTOCOL_KEYBOARD)),
    TUD_HID_REPORT_DESC_MOUSE(HID_REPORT_ID(HID_ITF_PROTOCOL_MOUSE)),
    TUD_HID_REPORT_DESC_CARDMIC_ID,
};
static char s_identity[CARDMIC_ID_LEN + 1] = "CM1;pair=0";

#ifndef CARDMIC_DEV_TOOLS
static const uint8_t s_configuration_descriptor[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, 0, USB_CONFIG_TOTAL_LEN, 0x00, 100),
    TUD_AUDIO_MIC_ONE_CH_DESCRIPTOR(ITF_NUM_AUDIO_CONTROL, STRID_AUDIO_INTERFACE, AUDIO_BYTES_PER_SAMPLE,
                                    AUDIO_BYTES_PER_SAMPLE * 8, USB_AUDIO_EP_IN, CFG_TUD_AUDIO_EP_SZ_IN),
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, STRID_ID_INTERFACE, HID_ITF_PROTOCOL_NONE, sizeof(s_hid_id_only),
                       USB_HID_EP_IN, 16, 50),
};

static const uint8_t s_configuration_descriptor_kb[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL, 0, USB_CONFIG_TOTAL_LEN, 0x00, 100),
    TUD_AUDIO_MIC_ONE_CH_DESCRIPTOR(ITF_NUM_AUDIO_CONTROL, STRID_AUDIO_INTERFACE, AUDIO_BYTES_PER_SAMPLE,
                                    AUDIO_BYTES_PER_SAMPLE * 8, USB_AUDIO_EP_IN, CFG_TUD_AUDIO_EP_SZ_IN),
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, STRID_HID_INTERFACE, HID_ITF_PROTOCOL_NONE, sizeof(s_hid_kb_and_id),
                       USB_HID_EP_IN, 16, 10),
};

#endif  // !CARDMIC_DEV_TOOLS

static const char *s_string_descriptor[] = {
    (const char[]){0x09, 0x04},
    "Cardmic",
    "Cardmic Microphone",
    s_usb_serial,
    "Cardmic Microphone",
    "Cardmic Talk Key",
    "Cardmic Dev Console",
    "Cardmic",
};

#ifdef CARDMIC_DEV_TOOLS
// Same two configurations plus a serial port (interfaces after the others).
#define USB_CONFIG_TOTAL_LEN_DEV (USB_CONFIG_TOTAL_LEN + TUD_CDC_DESC_LEN)

static const uint8_t s_configuration_descriptor_dev[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL + 2, 0, USB_CONFIG_TOTAL_LEN_DEV, 0x00, 100),
    TUD_AUDIO_MIC_ONE_CH_DESCRIPTOR(ITF_NUM_AUDIO_CONTROL, STRID_AUDIO_INTERFACE, AUDIO_BYTES_PER_SAMPLE,
                                    AUDIO_BYTES_PER_SAMPLE * 8, USB_AUDIO_EP_IN, CFG_TUD_AUDIO_EP_SZ_IN),
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, STRID_ID_INTERFACE, HID_ITF_PROTOCOL_NONE, sizeof(s_hid_id_only),
                       USB_HID_EP_IN, 16, 50),
    TUD_CDC_DESCRIPTOR(ITF_NUM_TOTAL, STRID_CDC_INTERFACE, USB_CDC_EP_NOTIF, 8, USB_CDC_EP_OUT, USB_CDC_EP_IN, 64),
};

static const uint8_t s_configuration_descriptor_dev_kb[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_NUM_TOTAL + 2, 0, USB_CONFIG_TOTAL_LEN_DEV, 0x00, 100),
    TUD_AUDIO_MIC_ONE_CH_DESCRIPTOR(ITF_NUM_AUDIO_CONTROL, STRID_AUDIO_INTERFACE, AUDIO_BYTES_PER_SAMPLE,
                                    AUDIO_BYTES_PER_SAMPLE * 8, USB_AUDIO_EP_IN, CFG_TUD_AUDIO_EP_SZ_IN),
    TUD_HID_DESCRIPTOR(ITF_NUM_HID, STRID_HID_INTERFACE, HID_ITF_PROTOCOL_NONE, sizeof(s_hid_kb_and_id),
                       USB_HID_EP_IN, 16, 10),
    TUD_CDC_DESCRIPTOR(ITF_NUM_TOTAL, STRID_CDC_INTERFACE, USB_CDC_EP_NOTIF, 8, USB_CDC_EP_OUT, USB_CDC_EP_IN, 64),
};

static QueueHandle_t s_dev_lines;  // command lines from the USB serial port

static void dev_cdc_reply(const char *line)
{
    tinyusb_cdcacm_write_queue(TINYUSB_CDC_ACM_0, (const uint8_t *)line, strlen(line));
    tinyusb_cdcacm_write_queue(TINYUSB_CDC_ACM_0, (const uint8_t *)"\r\n", 2);
    tinyusb_cdcacm_write_flush(TINYUSB_CDC_ACM_0, pdMS_TO_TICKS(200));
}

static void dev_cdc_rx(int itf, cdcacm_event_t *event)
{
    (void)event;
    static char line[160];
    static size_t len;
    uint8_t buf[64];
    size_t got = 0;
    while (tinyusb_cdcacm_read(itf, buf, sizeof(buf), &got) == ESP_OK && got > 0) {
        for (size_t i = 0; i < got; ++i) {
            if (buf[i] == '\n' || buf[i] == '\r') {
                if (len && s_dev_lines) {
                    line[len] = '\0';
                    xQueueSend(s_dev_lines, line, 0);
                }
                len = 0;
            } else if (len < sizeof(line) - 1) {
                line[len++] = (char)buf[i];
            }
        }
        got = 0;
    }
}

static void dev_cdc_task(void *arg)
{
    (void)arg;
    char line[160];
    while (1) {
        if (xQueueReceive(s_dev_lines, line, portMAX_DELAY) == pdTRUE) {
            cardmic_dev_command(line, dev_cdc_reply);
        }
    }
}

static void dev_cdc_start(void)
{
    if (!s_dev_lines) {
        s_dev_lines = xQueueCreate(8, 160);
        xTaskCreate(dev_cdc_task, "cardmic_devcdc", 6144, NULL, 3, NULL);
    }
    tinyusb_config_cdcacm_t acm = {
        .usb_dev = TINYUSB_USBDEV_0,
        .cdc_port = TINYUSB_CDC_ACM_0,
        .callback_rx = dev_cdc_rx,
    };
    tusb_cdc_acm_init(&acm);
    // Send the app's log output there too: while Cardmic owns USB the
    // serial/JTAG console is unavailable, and those logs are what explain a
    // failed Wi-Fi join or a crash.
    esp_tusb_init_console(TINYUSB_CDC_ACM_0);
}
#endif

// The mux that gives the internal USB PHY to TinyUSB lives in the RTC domain
// and survives esp_restart(). Without this, a software restart while Cardmic
// is open (after an OTA update, or a dev reboot into the ROM bootloader) comes
// back with no USB device at all, and the host keeps showing the stale
// microphone until the Cardputer is power-cycled.
static void give_phy_back_on_restart(void)
{
    if (s_installed) {
        tud_disconnect();
    }
    usb_serial_jtag_ll_phy_enable_external(false);  // internal PHY -> USB-Serial-JTAG
}

esp_err_t cardmic_usb_start(bool with_keyboard)
{
    if (s_installed) {
        return ESP_OK;
    }
    static bool s_restart_hook;
    if (!s_restart_hook) {
        s_restart_hook = esp_register_shutdown_handler(give_phy_back_on_restart) == ESP_OK;
    }
    s_keyboard = with_keyboard;
#ifdef CARDMIC_DEV_TOOLS
    s_device_descriptor.idProduct = with_keyboard ? USB_PID_DEV_MIC_KEYBOARD : USB_PID_DEV_MIC;
#else
    s_device_descriptor.idProduct = with_keyboard ? USB_PID_MIC_KEYBOARD : USB_PID_MIC;
#endif
    if (s_jtag_phy) {
        usb_del_phy(s_jtag_phy);
        s_jtag_phy = NULL;
    }

    for (size_t i = 0; i < AUDIO_CHANNEL_COUNT + 1; ++i) {
        s_mute[i] = false;
        s_volume[i] = 0;
    }
    s_volume_range.wNumSubRanges = 1;
    s_volume_range.subrange[0].bMin = -90;
    s_volume_range.subrange[0].bMax = 0;
    s_volume_range.subrange[0].bRes = 1;
    s_sample_freq_range.wNumSubRanges = 1;
    s_sample_freq_range.subrange[0].bMin = CARDMIC_SAMPLE_RATE_HZ;
    s_sample_freq_range.subrange[0].bMax = CARDMIC_SAMPLE_RATE_HZ;
    s_sample_freq_range.subrange[0].bRes = 0;

    uint8_t mac[6] = {0};
    if (esp_read_mac(mac, ESP_MAC_WIFI_STA) == ESP_OK) {
        snprintf(s_usb_serial, sizeof(s_usb_serial), "%02X%02X%02X%02X%02X%02X", mac[0], mac[1], mac[2], mac[3],
                 mac[4], mac[5]);
    }

    tinyusb_config_t cfg = {
        .device_descriptor = &s_device_descriptor,
        .string_descriptor = s_string_descriptor,
        .string_descriptor_count = sizeof(s_string_descriptor) / sizeof(s_string_descriptor[0]),
        .external_phy = false,
#ifdef CARDMIC_DEV_TOOLS
        .configuration_descriptor = with_keyboard ? s_configuration_descriptor_dev_kb : s_configuration_descriptor_dev,
#else
        .configuration_descriptor = with_keyboard ? s_configuration_descriptor_kb : s_configuration_descriptor,
#endif
    };
    esp_err_t err = tinyusb_driver_install(&cfg);
    if (err != ESP_OK) {
        ESP_LOGW(TAG, "tinyusb_driver_install: %s", esp_err_to_name(err));
        return err;
    }
    s_installed = true;
#ifdef CARDMIC_DEV_TOOLS
    dev_cdc_start();
#endif
    return ESP_OK;
}

void cardmic_usb_stop(void)
{
    if (!s_installed) {
        return;
    }
    s_streaming = false;
    tinyusb_driver_uninstall();
    s_installed = false;
    s_mounted = false;

    // Hand the internal PHY back to the USB-Serial-JTAG controller. Without
    // this, after leaving Cardmic the stock menu shows no USB device at all
    // until a reboot, which also blocks button-free USB updates.
    usb_phy_config_t jtag = {
        .controller = USB_PHY_CTRL_SERIAL_JTAG,
        .target = USB_PHY_TARGET_INT,
        .otg_mode = USB_OTG_MODE_DEVICE,
    };
    if (usb_new_phy(&jtag, &s_jtag_phy) != ESP_OK) {
        s_jtag_phy = NULL;
        ESP_LOGW(TAG, "could not restore USB-Serial-JTAG");
    }
}

bool cardmic_usb_mounted(void) { return s_installed && s_mounted; }
bool cardmic_usb_keyboard_ready(void) { return s_installed && s_keyboard && s_mounted && tud_hid_ready(); }

bool cardmic_usb_talk_key(uint8_t modifiers, uint8_t keycode, bool down)
{
    if (!s_installed || !s_keyboard || !s_mounted) {
        return false;
    }
    // The interrupt endpoint may still be busy with the previous report.
    for (int i = 0; i < 20 && !tud_hid_ready(); ++i) {
        vTaskDelay(pdMS_TO_TICKS(1));
    }
    uint8_t keys[6] = {down ? keycode : 0};
    return tud_hid_keyboard_report(HID_ITF_PROTOCOL_KEYBOARD, down ? modifiers : 0, keycode ? keys : NULL);
}
bool cardmic_usb_streaming(void) { return s_installed && s_streaming; }

void cardmic_usb_set_identity(const char *code, const char *name, const char *fw)
{
    char id[CARDMIC_ID_LEN + 1];
    if (code && *code) {
        snprintf(id, sizeof(id), "CM1;pair=1;code=%s;name=%s;fw=%s", code, name, fw);
    } else {
        snprintf(id, sizeof(id), "CM1;pair=0;name=%s;fw=%s", name, fw);
    }
    strlcpy(s_identity, id, sizeof(s_identity));
}

// Called from the stock HID helper's TinyUSB callbacks while Cardmic owns USB.
const uint8_t *cardmic_hid_report_descriptor(uint8_t instance)
{
    (void)instance;
    if (!s_installed) return NULL;
    return s_keyboard ? s_hid_kb_and_id : s_hid_id_only;
}

uint16_t cardmic_hid_get_report(uint8_t instance, uint8_t report_id, hid_report_type_t type, uint8_t *buffer,
                                uint16_t reqlen)
{
    (void)instance;
    if (!s_installed || report_id != CARDMIC_ID_REPORT || type != HID_REPORT_TYPE_FEATURE) return 0;
    uint16_t n = reqlen < CARDMIC_ID_LEN ? reqlen : CARDMIC_ID_LEN;
    memset(buffer, 0, n);
    memcpy(buffer, s_identity, strnlen(s_identity, n));
    return n;
}
bool cardmic_usb_host_muted(void) { return s_mute[0] || s_mute[1]; }

void cardmic_usb_write(const int16_t *samples, size_t count)
{
    if (s_installed && s_streaming && tud_ready() && tud_audio_mounted()) {
        (void)tud_audio_write(samples, count * sizeof(int16_t));
    }
}

/* ----------------------------- TinyUSB callbacks ----------------------------- */

void tud_mount_cb(void) { s_mounted = true; }

void tud_umount_cb(void)
{
    s_mounted = false;
    s_streaming = false;
}

bool tud_audio_set_itf_cb(uint8_t rhport, tusb_control_request_t const *request)
{
    (void)rhport;
    s_streaming = TU_U16_LOW(request->wValue) != 0;
    return true;
}

bool tud_audio_set_itf_close_ep_cb(uint8_t rhport, tusb_control_request_t const *request)
{
    (void)rhport;
    (void)request;
    s_streaming = false;
    tud_audio_clear_ep_in_ff();
    return true;
}

bool tud_audio_set_req_ep_cb(uint8_t rhport, tusb_control_request_t const *request, uint8_t *buffer)
{
    (void)rhport;
    (void)request;
    (void)buffer;
    return false;
}

bool tud_audio_set_req_itf_cb(uint8_t rhport, tusb_control_request_t const *request, uint8_t *buffer)
{
    (void)rhport;
    (void)request;
    (void)buffer;
    return false;
}

bool tud_audio_set_req_entity_cb(uint8_t rhport, tusb_control_request_t const *request, uint8_t *buffer)
{
    (void)rhport;
    uint8_t channel_num = TU_U16_LOW(request->wValue);
    uint8_t control_selector = TU_U16_HIGH(request->wValue);
    uint8_t entity_id = TU_U16_HIGH(request->wIndex);

    if (request->bRequest != AUDIO_CS_REQ_CUR || channel_num > AUDIO_CHANNEL_COUNT || entity_id != 2) {
        return false;
    }
    switch (control_selector) {
    case AUDIO_FU_CTRL_MUTE:
        if (request->wLength != sizeof(audio_control_cur_1_t)) {
            return false;
        }
        s_mute[channel_num] = ((audio_control_cur_1_t *)buffer)->bCur != 0;
        return true;
    case AUDIO_FU_CTRL_VOLUME:
        if (request->wLength != sizeof(audio_control_cur_2_t)) {
            return false;
        }
        s_volume[channel_num] = ((audio_control_cur_2_t *)buffer)->bCur;
        return true;
    default:
        return false;
    }
}

bool tud_audio_get_req_ep_cb(uint8_t rhport, tusb_control_request_t const *request)
{
    (void)rhport;
    (void)request;
    return false;
}

bool tud_audio_get_req_itf_cb(uint8_t rhport, tusb_control_request_t const *request)
{
    (void)rhport;
    (void)request;
    return false;
}

bool tud_audio_get_req_entity_cb(uint8_t rhport, tusb_control_request_t const *request)
{
    uint8_t channel_num = TU_U16_LOW(request->wValue);
    uint8_t control_selector = TU_U16_HIGH(request->wValue);
    uint8_t entity_id = TU_U16_HIGH(request->wIndex);

    if (channel_num > AUDIO_CHANNEL_COUNT) {
        return false;
    }

    if (entity_id == 1 && control_selector == AUDIO_TE_CTRL_CONNECTOR) {
        audio_desc_channel_cluster_t cluster = {
            .bNrChannels = AUDIO_CHANNEL_COUNT,
            .bmChannelConfig = AUDIO_CHANNEL_CONFIG_NON_PREDEFINED,
            .iChannelNames = 0,
        };
        return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &cluster, sizeof(cluster));
    }

    if (entity_id == 2) {
        switch (control_selector) {
        case AUDIO_FU_CTRL_MUTE:
            return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &s_mute[channel_num], 1);
        case AUDIO_FU_CTRL_VOLUME:
            if (request->bRequest == AUDIO_CS_REQ_CUR) {
                return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &s_volume[channel_num],
                                                                  sizeof(s_volume[channel_num]));
            }
            if (request->bRequest == AUDIO_CS_REQ_RANGE) {
                return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &s_volume_range,
                                                                  sizeof(s_volume_range));
            }
            return false;
        default:
            return false;
        }
    }

    if (entity_id == 4) {
        switch (control_selector) {
        case AUDIO_CS_CTRL_SAM_FREQ:
            if (request->bRequest == AUDIO_CS_REQ_CUR) {
                return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &s_sample_freq,
                                                                  sizeof(s_sample_freq));
            }
            if (request->bRequest == AUDIO_CS_REQ_RANGE) {
                return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &s_sample_freq_range,
                                                                  sizeof(s_sample_freq_range));
            }
            return false;
        case AUDIO_CS_CTRL_CLK_VALID:
            if (request->bRequest == AUDIO_CS_REQ_CUR) {
                return tud_audio_buffer_and_schedule_control_xfer(rhport, request, &s_clock_valid,
                                                                  sizeof(s_clock_valid));
            }
            return false;
        default:
            return false;
        }
    }

    return false;
}
