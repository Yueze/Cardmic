/*
 * Cardmic app for the stock Cardputer ADV firmware.
 *
 * Audio path (tuned on real hardware): ES8311 PGA +30 dB (REG14=0x1A), Q8 DC
 * blocker, 100 Hz 4th-order high-pass, selectable digital gain. The app owns
 * the I2S port while open; the stock speaker/mic are stopped on entry and the
 * speaker is restored on exit.
 *
 * Visual language: Rams / Teenage Engineering structure (black canvas, one
 * grid, small pixel caps for labels), a scrolling heat-map spectrogram of the
 * voice as the one expressive element, a segmented level meter with peak hold,
 * and a brief Matrix rain when the app opens. Laid out for the 204 x 109 app canvas --
 * the stock system bar and keyboard bar keep the rest of the screen.
 *
 * SPDX-License-Identifier: MIT
 */
#include "app_cardmic.h"
#include "cardmic_net.h"
#include "cardmic_ota.h"
#include "cardmic_pair.h"
#include "cardmic_usb.h"
#include "assets/cardmic_big.h"
#include "assets/cardmic_small.h"
#include <apps/utils/audio/audio.h>
#include <apps/utils/common.h>
#include <apps/utils/theme.h>
#include <mooncake_log.h>
#include <assets.h>
#include <hal.h>
#include <M5Unified.hpp>
#include <driver/i2s_pdm.h>
#include <driver/i2s_std.h>
#include <esp_app_desc.h>
#include <esp_netif.h>
#include <esp_ota_ops.h>
#include <esp_wifi.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>
#include <lwip/sockets.h>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <functional>
#include <string>
#include <vector>

using namespace mooncake;

namespace {

/* ------------------------------------------------------------------ palette */
constexpr uint32_t C_BG       = 0x000000;
constexpr uint32_t C_TEXT     = 0xF2F2F2;
constexpr uint32_t C_DIM      = 0x6E6E6E;
constexpr uint32_t C_FAINT    = 0x262626;
constexpr uint32_t C_ROW      = 0x121212;
constexpr uint32_t C_ACCENT   = 0x99FF00;  // stock system-bar lime
constexpr uint32_t C_CYAN     = 0x28F0FF;
constexpr uint32_t C_MAGENTA  = 0xFF3BD4;
constexpr uint32_t C_VIOLET   = 0x8A5CFF;
constexpr uint32_t C_WARN     = 0xFFB020;
constexpr uint32_t C_MUTE     = 0xFF4A4A;
constexpr uint32_t C_RAIN     = 0x00FF66;

uint32_t mix(uint32_t a, uint32_t b, float t)  // a -> b
{
    auto ch = [&](int s) {
        float x = ((a >> s) & 0xFF) * (1 - t) + ((b >> s) & 0xFF) * t;
        return (uint32_t)std::clamp(x, 0.0f, 255.0f) << s;
    };
    return ch(16) | ch(8) | ch(0);
}

uint32_t hash32(uint32_t x)
{
    x ^= x >> 16;
    x *= 0x7feb352d;
    x ^= x >> 15;
    x *= 0x846ca68b;
    x ^= x >> 16;
    return x;
}

/* -------------------------------------------------------------- audio state */
// The version lives in one place: PROJECT_VER in the top-level CMakeLists.txt.
const char* version() { return esp_app_get_description()->version; }

constexpr uint8_t ES8311_ADDR = 0x18;
constexpr float MIC_HPF_HZ    = 100.0f;
constexpr int32_t GAIN_MULT[] = {1, 2, 4};  // LOW / MID / HIGH (MID = tuned default)
// The ADV's ES8311 already amplifies by +30 dB (PGA); the original Cardputer's
// PDM microphone has no preamp, so its samples get this much more digital gain
// (M5Unified's default "magnification" for PDM mics). Not yet tuned on hardware.
constexpr int32_t PDM_BASE_GAIN = 16;
int32_t s_base_gain           = 1;
const char* const GAIN_NAME[] = {"LOW", "MID", "HIGH"};

enum class WifiState { NotConfigured, Connecting, Connected, Failed };

i2s_chan_handle_t s_rx          = nullptr;
TaskHandle_t s_audio_task       = nullptr;
volatile bool s_audio_run       = false;
volatile bool s_device_muted    = false;
volatile int s_gain_idx         = 1;
volatile uint16_t s_level       = 0;
volatile WifiState s_wifi_state = WifiState::NotConfigured;
std::string s_wifi_ssid;

// Recent processed samples (pre-mute) for the spectrogram.
constexpr int RING = 1024;
int16_t s_ring[RING];
volatile uint32_t s_ring_w = 0;

// Spectrogram: a sprite that scrolls one column left per frame.
constexpr int SPEC_W = 200, SPEC_H = 62, SPEC_X = 2, SPEC_Y = 14;
LGFX_Sprite* s_spec = nullptr;
float s_nf[SPEC_H];      // per-band noise floor, dB
bool s_nf_ready = false;

// Level meter ballistics and a slow numeric readout.
float s_meter      = 0.0f;  // 0..1, fast attack / slow release
float s_peak_hold  = 0.0f;
uint32_t s_peak_at = 0;
int s_db_shown     = -90;
uint32_t s_db_at   = 0;
uint32_t s_intro_at = 0;  // Matrix rain start

/* -------------------------------------------------------------------- pages */
enum class Page {
    Main,
    Settings,
    NetInfo,
    About,
    Pairing,
    Scanning,
    List,
    ManualSsid,
    Password,
    Connecting,
    ConnectFailed,
};
volatile Page s_page = Page::Main;

enum SettingItem { SET_WIFI, SET_NETWORK, SET_PAIRING, SET_TALK, SET_MUTE, SET_GAIN, SET_ABOUT, SET_COUNT };

// Push-to-talk ("voice keyboard"): holding SPACE on the main screen holds a
// key chord on the computer over USB, which dictation apps use as their
// hold-to-talk key. Modifier bits are HID: 0x01 Ctrl, 0x04 Alt/Option, 0x08 GUI.
enum TalkKey { TALK_OFF, TALK_CTRL_OPT, TALK_CTRL_WIN, TALK_F13, TALK_COUNT };
const struct {
    const char* name;
    uint8_t modifiers;
    uint8_t keycode;
} TALK_KEYS[TALK_COUNT] = {
    {"OFF", 0, 0},
    {"CTRL+OPT", 0x01 | 0x04, 0},  // macOS: Wispr Flow's hold key on keyboards without Fn
    {"CTRL+WIN", 0x01 | 0x08, 0},  // Windows: Wispr Flow's default
    {"F13", 0, 0x68},              // bind it to anything in your dictation app
};
int s_talk_key     = TALK_OFF;
bool s_talk_down   = false;
bool s_usb_ok      = false;
bool s_audio_ok    = false;

// Pairing (see cardmic_pair.h): the code and whether it is required.
char s_pair_code[CARDMIC_PAIR_CODE_LEN + 1] = "";
bool s_pair_required         = false;
volatile bool s_pair_busy    = false;
int s_set_sel = 0;

std::vector<std::pair<int, std::string>> s_scan;
int s_sel = 0;
std::string s_pick_ssid;
std::string s_entry;  // text being typed (SSID or password)
bool s_show_pass = false;
// Screenshots taken over Wi-Fi for the docs show made-up network details, so
// no real SSID or address ends up in a picture.
bool s_demo = false;

struct Biquad {
    float b0, b1, b2, a1, a2;
    float x1 = 0, x2 = 0, y1 = 0, y2 = 0;

    void highpass(float fc, float fs)
    {
        float w0    = 2.0f * (float)M_PI * fc / fs;
        float alpha = sinf(w0) / (2.0f * 0.70710678f);
        float c     = cosf(w0);
        float a0    = 1.0f + alpha;
        b0          = (1.0f + c) / 2.0f / a0;
        b1          = -(1.0f + c) / a0;
        b2          = (1.0f + c) / 2.0f / a0;
        a1          = -2.0f * c / a0;
        a2          = (1.0f - alpha) / a0;
    }

    float run(float x)
    {
        float y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        x2      = x1;
        x1      = x;
        y2      = y1;
        y1      = y;
        return y;
    }
};

// ES8311 ADC path for the on-board MEMS mic. REG14 = 0x1A is Espressif's
// reference "enable analog MIC and max PGA gain"; 0x10 (PGA 0 dB) measured
// ~30 dB too quiet for dictation on this hardware.
bool es8311_enable_mic()
{
    static const uint8_t seq[][2] = {
        {0x00, 0x80}, {0x01, 0xBA}, {0x02, 0x18}, {0x0D, 0x01},
        {0x0E, 0x02}, {0x14, 0x1A}, {0x17, 0xBF}, {0x1C, 0x6A},
    };
    for (size_t i = 0; i < sizeof(seq) / sizeof(seq[0]); ++i) {
        if (!M5.In_I2C.writeRegister8(ES8311_ADDR, seq[i][0], seq[i][1], 400000)) {
            return false;
        }
        if (i == 0) {
            vTaskDelay(pdMS_TO_TICKS(10));
        }
    }
    return true;
}

bool i2s_mic_start()
{
    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_AUTO, I2S_ROLE_MASTER);
    chan_cfg.dma_desc_num      = 8;
    chan_cfg.dma_frame_num     = 64;
    if (i2s_new_channel(&chan_cfg, nullptr, &s_rx) != ESP_OK) {
        return false;
    }
    i2s_std_config_t std_cfg = {
        .clk_cfg  = I2S_STD_CLK_DEFAULT_CONFIG(CARDMIC_SAMPLE_RATE_HZ),
        .slot_cfg = I2S_STD_PHILIPS_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_16BIT, I2S_SLOT_MODE_MONO),
        .gpio_cfg =
            {
                .mclk = I2S_GPIO_UNUSED,
                .bclk = GPIO_NUM_41,
                .ws   = GPIO_NUM_43,
                .dout = I2S_GPIO_UNUSED,
                .din  = GPIO_NUM_46,
                .invert_flags = {.mclk_inv = false, .bclk_inv = false, .ws_inv = false},
            },
    };
    std_cfg.clk_cfg.clk_src         = I2S_CLK_SRC_PLL_160M;
    std_cfg.clk_cfg.mclk_multiple   = I2S_MCLK_MULTIPLE_128;
    std_cfg.slot_cfg.slot_mask      = I2S_STD_SLOT_RIGHT;
    std_cfg.slot_cfg.slot_bit_width = I2S_SLOT_BIT_WIDTH_16BIT;
    if (i2s_channel_init_std_mode(s_rx, &std_cfg) != ESP_OK || i2s_channel_enable(s_rx) != ESP_OK) {
        i2s_del_channel(s_rx);
        s_rx = nullptr;
        return false;
    }
    return true;
}

// Original Cardputer: SPM1423 PDM microphone, clock on G43, data on G46 (the
// same pins the ADV uses for I2S WS/DIN). PDM RX exists only on I2S0.
bool pdm_mic_start()
{
    i2s_chan_config_t chan_cfg = I2S_CHANNEL_DEFAULT_CONFIG(I2S_NUM_0, I2S_ROLE_MASTER);
    chan_cfg.dma_desc_num      = 8;
    chan_cfg.dma_frame_num     = 64;
    if (i2s_new_channel(&chan_cfg, nullptr, &s_rx) != ESP_OK) {
        return false;
    }
    i2s_pdm_rx_config_t pdm_cfg = {
        .clk_cfg  = I2S_PDM_RX_CLK_DEFAULT_CONFIG(CARDMIC_SAMPLE_RATE_HZ),
        .slot_cfg = I2S_PDM_RX_SLOT_DEFAULT_CONFIG(I2S_DATA_BIT_WIDTH_16BIT, I2S_SLOT_MODE_MONO),
        .gpio_cfg =
            {
                .clk          = GPIO_NUM_43,
                .din          = GPIO_NUM_46,
                .invert_flags = {.clk_inv = false},
            },
    };
    // M5Unified reads this microphone from the right slot (its default
    // input_only_right, not overridden for the Cardputer); the IDF default is left.
    pdm_cfg.slot_cfg.slot_mask = I2S_PDM_SLOT_RIGHT;
    if (i2s_channel_init_pdm_rx_mode(s_rx, &pdm_cfg) != ESP_OK || i2s_channel_enable(s_rx) != ESP_OK) {
        i2s_del_channel(s_rx);
        s_rx = nullptr;
        return false;
    }
    return true;
}

void i2s_mic_stop()
{
    if (s_rx) {
        i2s_channel_disable(s_rx);
        i2s_del_channel(s_rx);
        s_rx = nullptr;
    }
}

void audio_task(void*)
{
    int16_t frame[CARDMIC_SAMPLES_PER_MS];
    int32_t dc_acc = 0;  // DC estimate x256 (Q8): no truncation dead band
    Biquad hpf[2];
    hpf[0].highpass(MIC_HPF_HZ, (float)CARDMIC_SAMPLE_RATE_HZ);
    hpf[1].highpass(MIC_HPF_HZ, (float)CARDMIC_SAMPLE_RATE_HZ);
    uint16_t envelope = 0;

    while (s_audio_run) {
        size_t got = 0;
        if (i2s_channel_read(s_rx, frame, sizeof(frame), &got, pdMS_TO_TICKS(100)) != ESP_OK || got != sizeof(frame)) {
            continue;
        }
        const int32_t gain = GAIN_MULT[s_gain_idx] * s_base_gain;
        uint16_t peak      = 0;
        for (int i = 0; i < CARDMIC_SAMPLES_PER_MS; ++i) {
            int32_t raw = frame[i];
            dc_acc += raw - (dc_acc >> 8);
            float y   = hpf[1].run(hpf[0].run((float)(raw - (dc_acc >> 8))));
            int32_t s = std::clamp<int32_t>((int32_t)(y * gain), INT16_MIN, INT16_MAX);
            frame[i]  = (int16_t)s;
            peak      = std::max<uint16_t>(peak, (uint16_t)(s < 0 ? -s : s));
        }
        uint32_t w = s_ring_w;
        for (int i = 0; i < CARDMIC_SAMPLES_PER_MS; ++i) s_ring[(w + i) % RING] = frame[i];
        s_ring_w = w + CARDMIC_SAMPLES_PER_MS;
        envelope = peak >= envelope ? peak : envelope - envelope / 24 - 1;
        s_level  = envelope;

        // Device mute applies to both outputs; the prototype only muted USB.
        if (s_device_muted || cardmic_usb_host_muted()) {
            memset(frame, 0, sizeof(frame));
        }
        cardmic_usb_write(frame, CARDMIC_SAMPLES_PER_MS);
        cardmic_net_push(frame, CARDMIC_SAMPLES_PER_MS);
    }
    s_audio_task = nullptr;
    vTaskDelete(nullptr);
}

/* ------------------------------------------------------------- WiFi tasks */

// HAL::wifiConnect blocks for up to ~13 s, so all of this runs off the UI loop.
volatile bool s_autoconnect_running = false;

void autoconnect_task(void*)
{
    std::string pass = GetHAL().getSettings().GetString("wifi_password", "");
    bool ok          = GetHAL().isWifiConnected() || GetHAL().wifiConnect(s_wifi_ssid, pass);
    s_wifi_state     = ok ? WifiState::Connected : WifiState::Failed;
    if (ok) {
        cardmic_net_start();
    }
    s_autoconnect_running = false;
    vTaskDelete(nullptr);
}

void start_autoconnect()
{
    if (s_autoconnect_running || s_wifi_ssid.empty()) return;
    s_autoconnect_running = true;
    s_wifi_state          = WifiState::Connecting;
    if (xTaskCreate(autoconnect_task, "cardmic_wifi", 6144, nullptr, 4, nullptr) != pdPASS) {
        s_autoconnect_running = false;
        s_wifi_state          = WifiState::Failed;
    }
}

// Keeps the WIFI light honest: notices a dropped network (and rejoins), and a
// network brought up elsewhere. Retries a failed join every 30 s.
void sync_wifi_state()
{
    static uint32_t last_check = 0, lost_at = 0, failed_at = 0;
    const uint32_t now = GetHAL().millis();
    if (now - last_check < 500 || s_page == Page::Connecting || s_page == Page::Scanning) return;
    last_check    = now;
    const bool up = GetHAL().isWifiConnected();
    switch (s_wifi_state) {
        case WifiState::Connected:
            if (up) {
                lost_at = 0;
            } else if (!lost_at) {
                lost_at = now;
            } else if (now - lost_at > 3000) {
                lost_at = 0;
                start_autoconnect();
            }
            break;
        case WifiState::Failed:
            if (up) {
                s_wifi_state = WifiState::Connected;
                cardmic_net_start();
            } else if (!failed_at) {
                failed_at = now;
            } else if (now - failed_at > 30000) {
                failed_at = 0;
                start_autoconnect();
            }
            break;
        case WifiState::NotConfigured:
            if (up) {
                s_wifi_state = WifiState::Connected;
                cardmic_net_start();
            }
            break;
        case WifiState::Connecting:
            break;
    }
}

void scan_task(void*)
{
    // HAL::wifiScan does not bring WiFi up itself; the stock Scan app calls
    // wifiInit() first. Without it the scan fails and the list comes back empty.
    GetHAL().wifiInit();
    std::vector<std::pair<int, std::string>> found;
    GetHAL().wifiScan(found);  // sorted strongest first, hidden SSIDs dropped
    s_scan.clear();
    for (auto& r : found) {
        bool dup = false;
        for (auto& e : s_scan) dup |= (e.second == r.second);
        if (!dup) s_scan.push_back(r);
    }
    s_sel  = 0;
    s_page = Page::List;
    vTaskDelete(nullptr);
}

void join_task(void*)
{
    s_wifi_state = WifiState::Connecting;
    bool ok      = GetHAL().wifiConnect(s_pick_ssid, s_entry);
    if (ok) {
        GetHAL().getSettings().SetString("wifi_ssid", s_pick_ssid);
        GetHAL().getSettings().SetString("wifi_password", s_entry);
        s_wifi_ssid  = s_pick_ssid;
        s_wifi_state = WifiState::Connected;
        cardmic_net_start();
        s_page = Page::Settings;
    } else {
        s_wifi_state = s_wifi_ssid.empty() ? WifiState::NotConfigured : WifiState::Failed;
        s_page       = Page::ConnectFailed;
    }
    vTaskDelete(nullptr);
}

std::string device_ip()
{
    if (s_demo) return "192.168.1.42";
    esp_netif_t* nif = esp_netif_get_handle_from_ifkey("WIFI_STA_DEF");
    esp_netif_ip_info_t ip;
    if (!nif || esp_netif_get_ip_info(nif, &ip) != ESP_OK || ip.ip.addr == 0) return "";
    char buf[16];
    snprintf(buf, sizeof(buf), IPSTR, IP2STR(&ip.ip));
    return buf;
}

/* ------------------------------------------------------------------- input */

bool type_into(std::string& buf, const Keyboard::KeyEvent_t& e, size_t max_len)
{
    if (e.keyCode == KEY_BACKSPACE) {
        if (!buf.empty()) buf.pop_back();
        return true;
    }
    if (e.keyCode == KEY_SPACE) {
        if (buf.size() < max_len) buf += ' ';
        return true;
    }
    if (e.keyName && strlen(e.keyName) == 1 && buf.size() < max_len) {
        buf += e.keyName;
        return true;
    }
    return false;
}

void talk(bool down)
{
    if (down == s_talk_down) return;
    s_talk_down = down;
    if (s_talk_key != TALK_OFF) {
        cardmic_usb_talk_key(TALK_KEYS[s_talk_key].modifiers, TALK_KEYS[s_talk_key].keycode, down);
    }
}

void start_audio()
{
    if (!s_audio_ok || s_audio_task) return;
    s_audio_run = true;
    xTaskCreatePinnedToCore(audio_task, "cardmic_audio", 4096, nullptr, 7, &s_audio_task, 0);
}

void stop_audio()
{
    s_audio_run = false;
    for (int i = 0; i < 50 && s_audio_task; ++i) vTaskDelay(pdMS_TO_TICKS(10));
}

// Re-enumerate USB, with or without the talk-key keyboard. The audio task
// writes to USB, so it is paused around the switch.
void restart_usb()
{
    talk(false);
    stop_audio();
    cardmic_usb_stop();
    s_usb_ok = cardmic_usb_start(s_talk_key != TALK_OFF) == ESP_OK;
    start_audio();
}

void pair_apply_task(void*)
{
    uint8_t enc[16], mac[16];
    bool ok = cardmic_pair_derive(s_pair_code, enc, mac);
    cardmic_net_set_pairing(s_pair_required && ok, enc, mac);
    memset(enc, 0, sizeof(enc));
    memset(mac, 0, sizeof(mac));
    s_pair_busy = false;
    vTaskDelete(nullptr);
}

// Hand the current pairing state to the network side. Deriving the keys
// takes a moment, so it runs in its own task.
void pair_apply()
{
    if (!s_pair_required) {
        cardmic_net_set_pairing(false, nullptr, nullptr);
        return;
    }
    s_pair_busy = true;
    if (xTaskCreate(pair_apply_task, "cardmic_pair", 6144, nullptr, 3, nullptr) != pdPASS) s_pair_busy = false;
}

void pair_load()
{
    std::string code = GetHAL().getSettings().GetString("cardmic_code", "");
    if (cardmic_pair_valid(code.c_str())) {
        strlcpy(s_pair_code, code.c_str(), sizeof(s_pair_code));
    } else {
        cardmic_pair_generate(s_pair_code);
        GetHAL().getSettings().SetString("cardmic_code", s_pair_code);
    }
    s_pair_required = GetHAL().getSettings().GetString("cardmic_pair", "0") == "1";
#ifdef CARDMIC_DEV_TOOLS
    cardmic_net_dev_set_code(s_pair_code);
#endif
    pair_apply();
}

void open_setting(int item)
{
    switch (item) {
        case SET_WIFI:
            s_page = Page::Scanning;
            xTaskCreate(scan_task, "cardmic_scan", 6144, nullptr, 4, nullptr);
            break;
        case SET_NETWORK:
            s_page = Page::NetInfo;
            break;
        case SET_PAIRING:
            s_page = Page::Pairing;
            break;
        case SET_TALK:
            s_talk_key = (s_talk_key + 1) % TALK_COUNT;
            GetHAL().getSettings().SetString("cardmic_talkkey", std::to_string(s_talk_key));
            restart_usb();
            break;
        case SET_MUTE:
            s_device_muted = !s_device_muted;
            break;
        case SET_GAIN:
            s_gain_idx = (s_gain_idx + 1) % 3;
            GetHAL().getSettings().SetString("cardmic_gain", std::to_string((int)s_gain_idx));
            break;
        case SET_ABOUT:
            s_page = Page::About;
            break;
    }
}

// One level up, used by both ESC and the G0 home button. Returns false on the
// main page (the caller then closes the app).
bool go_back()
{
    switch (s_page) {
        case Page::Main:
            return false;
        case Page::Settings:
            s_page = Page::Main;
            break;
        case Page::Scanning:
        case Page::Connecting:
            break;  // busy; ignore
        case Page::About:
            if (cardmic_ota_state() != CARDMIC_OTA_DOWNLOADING && cardmic_ota_state() != CARDMIC_OTA_DONE) {
                cardmic_ota_dismiss();
                s_page = Page::Settings;
            }
            break;
        case Page::Password:
        case Page::ManualSsid:
        case Page::ConnectFailed:
            s_page = Page::List;
            break;
        default:
            s_page = Page::Settings;
            break;
    }
    return true;
}

void handle_key(const Keyboard::KeyEvent_t& e)
{
    if (e.keyCode == KEY_SPACE && s_page == Page::Main && !e.isModifier) {
        talk(e.state);  // press and release both matter here
        return;
    }
    if (e.isModifier || !e.state) return;
    const char* k = e.keyName ? e.keyName : "";
    const bool up = e.keyCode == KEY_UP || e.keyCode == KEY_SEMICOLON;
    const bool dn = e.keyCode == KEY_DOWN || e.keyCode == KEY_DOT;

    if (e.keyCode == KEY_ESC) {
        go_back();
        return;
    }

    switch (s_page) {
        case Page::Main:
            if (!strcmp(k, "m") || !strcmp(k, "M")) {
                s_device_muted = !s_device_muted;
            } else if (!strcmp(k, "s") || !strcmp(k, "S") || e.keyCode == KEY_ENTER) {
                s_page = Page::Settings;
            }
            break;

        case Page::Settings:
            if (up && s_set_sel > 0) s_set_sel--;
            else if (dn && s_set_sel + 1 < SET_COUNT) s_set_sel++;
            else if (e.keyCode == KEY_ENTER) open_setting(s_set_sel);
            break;

        case Page::Pairing:
            if (e.keyCode == KEY_ENTER && !s_pair_busy) {
                s_pair_required = !s_pair_required;
                GetHAL().getSettings().SetString("cardmic_pair", s_pair_required ? "1" : "0");
                pair_apply();
            } else if ((!strcmp(k, "n") || !strcmp(k, "N")) && !s_pair_busy) {
                cardmic_pair_generate(s_pair_code);
                GetHAL().getSettings().SetString("cardmic_code", s_pair_code);
#ifdef CARDMIC_DEV_TOOLS
                cardmic_net_dev_set_code(s_pair_code);
#endif
                pair_apply();
            }
            break;

        case Page::About:
            if (e.keyCode != KEY_ENTER) break;
            if (cardmic_ota_state() == CARDMIC_OTA_AVAILABLE) cardmic_ota_install();
            else if (GetHAL().isWifiConnected()) cardmic_ota_check();
            break;

        case Page::List: {
            int items = (int)s_scan.size() + 1;  // + "Other network..."
            if (up && s_sel > 0) {
                s_sel--;
            } else if (dn && s_sel + 1 < items) {
                s_sel++;
            } else if (e.keyCode == KEY_ENTER) {
                s_entry.clear();
                s_show_pass = false;
                if (s_sel < (int)s_scan.size()) {
                    s_pick_ssid = s_scan[s_sel].second;
                    s_page      = Page::Password;
                } else {
                    s_page = Page::ManualSsid;
                }
            }
            break;
        }

        case Page::ManualSsid:
            if (e.keyCode == KEY_ENTER && !s_entry.empty()) {
                s_pick_ssid = s_entry;
                s_entry.clear();
                s_page = Page::Password;
            } else {
                type_into(s_entry, e, 32);
            }
            break;

        case Page::Password:
        case Page::ConnectFailed:
            if (e.keyCode == KEY_ENTER) {
                s_page = Page::Connecting;
                xTaskCreate(join_task, "cardmic_join", 6144, nullptr, 4, nullptr);
            } else if (e.keyCode == KEY_TAB) {
                s_show_pass = !s_show_pass;
            } else if (type_into(s_entry, e, 63)) {
                s_page = Page::Password;
            }
            break;

        default:
            break;
    }
}

/* ----------------------------------------------------------------- drawing */

// The stock firmware keeps a system bar (top, shows WiFi/battery) and a
// keyboard bar (left) on screen; an app only owns this 204 x 109 canvas.
constexpr int W = 204, H = 109;

LGFX_Sprite& C() { return GetHAL().canvas; }

void text(const lgfx::IFont* font, uint32_t color, int x, int y, const char* s,
          textdatum_t datum = textdatum_t::top_left)
{
    auto& c = C();
    c.setFont(font);
    c.setTextSize(1);
    c.setTextColor(color);
    c.setTextDatum(datum);
    c.drawString(s, x, y);
    c.setTextDatum(textdatum_t::top_left);
}

int text_w(const lgfx::IFont* font, const char* s)
{
    C().setFont(font);
    return C().textWidth(s);
}

// Pixel key-cap: white cap with black glyphs, then a dim label. Returns the
// x just past the label.
int keycap(int x, int y, const char* key, const char* label)
{
    int kw = text_w(&fonts::Font0, key) + 5;
    C().fillRoundRect(x, y, kw, 10, 1, C_TEXT);
    text(&fonts::Font0, C_BG, x + 3, y + 1, key);
    x += kw + 3;
    text(&fonts::Font0, C_DIM, x, y + 1, label);
    return x + text_w(&fonts::Font0, label) + 7;
}

void hint_row(std::initializer_list<std::pair<const char*, const char*>> hints)
{
    int x = 2;
    for (auto& h : hints) x = keycap(x, 98, h.first, h.second);
}

void page_title(const char* t, const char* right = nullptr)
{
    text(&fonts::Font0, C_ACCENT, 2, 2, t);
    if (right) text(&fonts::Font0, C_DIM, W - 2, 2, right, textdatum_t::top_right);
    C().drawFastHLine(0, 12, W, C_FAINT);
}

void signal_bars(int x, int y, int rssi, uint32_t on, uint32_t off)
{
    int n = rssi > -55 ? 4 : rssi > -67 ? 3 : rssi > -75 ? 2 : 1;
    for (int i = 0; i < 4; ++i) {
        int h = 3 + i * 2;
        C().fillRect(x + i * 3, y + 9 - h, 2, h, i < n ? on : off);
    }
}

void spinner(int cx, int cy, int r)
{
    float a = (float)((GetHAL().millis() / 3) % 360);
    C().fillArc(cx, cy, r - 2, r, 0, 360, C_FAINT);
    C().fillArc(cx, cy, r - 2, r, a, a + 70, C_CYAN);
    C().fillArc(cx, cy, r - 2, r, a + 120, a + 190, C_MAGENTA);
    C().fillArc(cx, cy, r - 2, r, a + 240, a + 310, C_ACCENT);
}

void text_field(int y, const std::string& value, bool mask)
{
    auto& c = C();
    c.fillRect(2, y, W - 4, 22, C_ROW);
    c.drawFastHLine(2, y + 21, W - 4, C_ACCENT);
    std::string shown = mask ? std::string(value.size(), '*') : value;
    while (!shown.empty() && text_w(&fonts::efontCN_16, shown.c_str()) > W - 16) shown.erase(0, 1);
    text(&fonts::efontCN_16, C_TEXT, 6, y + 3, shown.c_str());
    int cx = 6 + text_w(&fonts::efontCN_16, shown.c_str()) + 1;
    if ((GetHAL().millis() / 500) % 2 == 0) c.fillRect(cx, y + 4, 2, 14, C_ACCENT);
}

// Matrix-style rain shown for a moment when the app opens.
bool draw_intro()
{
    constexpr uint32_t DUR = 850;
    uint32_t t             = GetHAL().millis() - s_intro_at;
    if (t >= DUR) return false;
    static const char GLYPHS[] = "0123456789ABCDEF<>=+*:";
    constexpr int COLS = W / 6, ROWS = H / 8 + 1;
    float fade = t > DUR - 250 ? (DUR - t) / 250.0f : 1.0f;
    for (int col = 0; col < COLS; ++col) {
        uint32_t hc = hash32(col * 2654435761u);
        float speed = 0.018f + (hc % 13) * 0.0035f;  // rows per ms
        int head    = (int)(t * speed + (hc >> 8) % ROWS) % (ROWS + 7);
        for (int trail = 0; trail < 7; ++trail) {
            int row = head - trail;
            if (row < 0 || row >= ROWS) continue;
            char g[2]    = {GLYPHS[hash32(col * 131 + row * 7 + t / 70) % (sizeof(GLYPHS) - 1)], 0};
            uint32_t col_c = trail == 0 ? 0xC8FFD8 : mix(C_RAIN, C_BG, trail / 7.0f);
            text(&fonts::Font0, mix(C_BG, col_c, fade), col * 6, row * 8, g);
        }
    }
    return true;
}

// Heat-map colour for a distance from the waveform's centre line, 0..1:
// deep blue at the centre through cyan and lime to yellow and red at the tips.
// Spectrogram palette: black for "nothing above the noise floor", then the
// Cardmic heat ramp at full saturation.
uint32_t spec_color(float t)
{
    static const struct {
        float at;
        uint32_t col;
    } STOPS[] = {
        {0.00f, 0x000000}, {0.14f, 0x0D1FB0}, {0.32f, 0x2F6BFF}, {0.50f, 0x28F0FF},
        {0.68f, C_ACCENT}, {0.84f, 0xFFD000}, {1.00f, 0xFF3B3B},
    };
    t = std::clamp(t, 0.0f, 1.0f);
    for (size_t i = 1; i < sizeof(STOPS) / sizeof(STOPS[0]); ++i) {
        if (t <= STOPS[i].at) {
            float u = (t - STOPS[i - 1].at) / (STOPS[i].at - STOPS[i - 1].at);
            return mix(STOPS[i - 1].col, STOPS[i].col, u);
        }
    }
    return STOPS[6].col;
}

uint32_t heat(float t)
{
    static const struct {
        float at;
        uint32_t col;
    } STOPS[] = {
        {0.00f, 0x1B2A6B}, {0.22f, 0x2F6BFF}, {0.42f, 0x28F0FF},
        {0.62f, C_ACCENT}, {0.80f, 0xFFD000}, {1.00f, 0xFF3B3B},
    };
    t = std::clamp(t, 0.0f, 1.0f);
    for (size_t i = 1; i < sizeof(STOPS) / sizeof(STOPS[0]); ++i) {
        if (t <= STOPS[i].at) {
            float u = (t - STOPS[i - 1].at) / (STOPS[i].at - STOPS[i - 1].at);
            return mix(STOPS[i - 1].col, STOPS[i].col, u);
        }
    }
    return STOPS[5].col;
}

/* ------------------------------------------------------------ spectrogram */

constexpr int FFT_N = 256;  // 16 ms at 16 kHz; 62.5 Hz per bin
float s_tw_cos[FFT_N / 2], s_tw_sin[FFT_N / 2], s_hann[FFT_N];
int s_row_lo[SPEC_H], s_row_hi[SPEC_H];
bool s_fft_ready = false;

void fft_setup()
{
    if (s_fft_ready) return;
    for (int i = 0; i < FFT_N / 2; ++i) {
        s_tw_cos[i] = cosf(2.0f * (float)M_PI * i / FFT_N);
        s_tw_sin[i] = -sinf(2.0f * (float)M_PI * i / FFT_N);
    }
    for (int i = 0; i < FFT_N; ++i) s_hann[i] = 0.5f - 0.5f * cosf(2.0f * (float)M_PI * i / (FFT_N - 1));
    // Log-frequency rows, 100 Hz (bottom) .. 8 kHz (top).
    const float bin_hz = CARDMIC_SAMPLE_RATE_HZ / (float)FFT_N;
    for (int r = 0; r < SPEC_H; ++r) {
        float f0    = 100.0f * powf(80.0f, r / (float)SPEC_H);
        float f1    = 100.0f * powf(80.0f, (r + 1) / (float)SPEC_H);
        s_row_lo[r] = std::clamp((int)(f0 / bin_hz), 1, FFT_N / 2 - 1);
        s_row_hi[r] = std::clamp((int)(f1 / bin_hz), s_row_lo[r], FFT_N / 2 - 1);
    }
    s_fft_ready = true;
}

// In-place iterative radix-2 FFT.
void fft(float* re, float* im)
{
    for (int i = 1, j = 0; i < FFT_N; ++i) {
        int bit = FFT_N >> 1;
        for (; j & bit; bit >>= 1) j ^= bit;
        j ^= bit;
        if (i < j) {
            std::swap(re[i], re[j]);
            std::swap(im[i], im[j]);
        }
    }
    for (int len = 2; len <= FFT_N; len <<= 1) {
        int step = FFT_N / len;
        for (int i = 0; i < FFT_N; i += len) {
            for (int k = 0; k < len / 2; ++k) {
                float wr = s_tw_cos[k * step], wi = s_tw_sin[k * step];
                int a = i + k, b = i + k + len / 2;
                float xr = re[b] * wr - im[b] * wi;
                float xi = re[b] * wi + im[b] * wr;
                re[b]    = re[a] - xr;
                im[b]    = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
    }
}

// Analyse the newest 16 ms and paint it as the right-most column.
// One column per 10 ms of audio (the classic speech-analysis hop), so the
// picture moves at 100 px/s and the 200 px panel shows the last 2 seconds:
// syllables read as distinct blobs. Columns follow audio time, not frame
// rate, so a slow frame never stretches or squeezes the picture.
constexpr uint32_t SPEC_HOP = CARDMIC_SAMPLE_RATE_HZ / 100;
uint32_t s_col_pos = 0;  // ring index (monotonic) where the last column ended

void spectrogram_column(uint32_t end, int x, bool live, bool muted)
{
    static float re[FFT_N], im[FFT_N];
    for (int i = 0; i < FFT_N; ++i) {
        re[i] = s_ring[(end - FFT_N + i) % RING] * s_hann[i];
        im[i] = 0.0f;
    }
    fft(re, im);

    float db[SPEC_H];
    const float ref = FFT_N * 0.5f * 0.5f * 32768.0f;  // full-scale sine with Hann
    for (int r = 0; r < SPEC_H; ++r) {
        float m = 0.0f;
        for (int k = s_row_lo[r]; k <= s_row_hi[r]; ++k) m = std::max(m, re[k] * re[k] + im[k] * im[k]);
        db[r] = 10.0f * log10f(m / (ref * ref) + 1e-12f);
    }

    // Show each band relative to its own noise floor, so steady background
    // noise (hiss, hum, the room) is black and only sound above it lights up.
    // The floor follows a quiet band down quickly and creeps up slowly
    // (1.5 dB/s), so sustained speech is not mistaken for noise.
    for (int r = 0; r < SPEC_H; ++r) {
        if (!s_nf_ready) {
            s_nf[r] = db[r];
        } else if (db[r] < s_nf[r]) {
            s_nf[r] += (db[r] - s_nf[r]) * 0.25f;
        } else {
            s_nf[r] += 0.015f;
        }
        float above = db[r] - s_nf[r] - 7.0f;  // gate: noise flickers a few dB above its floor
        float t     = std::clamp(above / 38.0f, 0.0f, 1.0f);
        uint32_t col = muted ? mix(C_BG, 0x7A2424, t) : spec_color(t);
        s_spec->drawPixel(x, SPEC_H - 1 - r, col);
    }
    s_nf_ready = true;
    (void)live;
}

void spectrogram_step(bool live, bool muted)
{
    if (!s_spec) return;
    fft_setup();
    const uint32_t w = s_ring_w;
    // Fell further behind than the ring holds (e.g. after a busy page): skip ahead.
    if (w - s_col_pos > RING - FFT_N) s_col_pos = w - (RING - FFT_N);
    int n = (int)((w - s_col_pos) / SPEC_HOP);
    n     = std::min(n, SPEC_W);
    if (n <= 0) return;
    s_spec->scroll(-n, 0);
    for (int i = 0; i < n; ++i) {
        s_col_pos += SPEC_HOP;
        spectrogram_column(s_col_pos, SPEC_W - n + i, live, muted);
    }
}

void draw_spectrogram(bool live, bool muted)
{
    if (!s_spec) return;
    spectrogram_step(live, muted);
    s_spec->pushSprite(&C(), SPEC_X, SPEC_Y);
    // Log-frequency ticks, dim, on the left edge.
    auto tick = [](float hz, const char* label) {
        int r = (int)(SPEC_H * logf(hz / 100.0f) / logf(80.0f));
        int y = SPEC_Y + SPEC_H - 1 - r;
        C().drawFastHLine(SPEC_X, y, 3, C_DIM);
        text(&fonts::Font0, C_DIM, SPEC_X + 5, y - 3, label);
    };
    tick(4000, "4K");
    tick(1000, "1K");
    tick(250, "250");
}

// TE-style segmented level meter with a peak-hold tick, plus a slow readout.
void draw_meter(int x, int y, bool live, bool muted)
{
    auto& c        = C();
    const int SEGS = 16, SW = 4, SGAP = 1;
    float lvl_db   = 20.0f * log10f(std::max<float>(s_level, 1.0f) / 32768.0f);
    float target   = std::clamp((lvl_db + 60.0f) / 60.0f, 0.0f, 1.0f);  // -60..0 dBFS
    s_meter        = target > s_meter ? target : s_meter * 0.90f + target * 0.10f;
    uint32_t now   = GetHAL().millis();
    if (s_meter >= s_peak_hold || now - s_peak_at > 1000) {
        s_peak_hold = s_meter >= s_peak_hold ? s_meter : std::max(s_meter, s_peak_hold - 0.02f);
        if (s_meter >= s_peak_hold) s_peak_at = now;
    }
    int lit  = (int)lroundf(s_meter * SEGS);
    int peak = std::min(SEGS - 1, (int)lroundf(s_peak_hold * SEGS));
    for (int i = 0; i < SEGS; ++i) {
        uint32_t on  = muted ? 0x6A1F1F : heat((i + 0.5f) / SEGS);
        uint32_t col = i < lit ? on : C_FAINT;
        (void)live;
        if (i == peak && peak > 0 && !muted) col = C_TEXT;
        c.fillRect(x + i * (SW + SGAP), y, SW, 8, col);
    }
    if (now - s_db_at > 250) {  // 4 Hz: readable, no flicker
        s_db_shown = (int)lroundf(s_meter * 60.0f - 60.0f);  // meter maps -60..0 dBFS linearly
        s_db_at    = now;
    }
    char buf[12];
    if (muted) {
        snprintf(buf, sizeof(buf), "MUTED");
    } else {
        snprintf(buf, sizeof(buf), "%d DB", s_db_shown);
    }
    text(&fonts::Font0, muted ? C_MUTE : C_DIM, x + SEGS * (SW + SGAP) - SGAP, y + 12, buf, textdatum_t::top_right);
}

void draw_main(bool usb_ok)
{
    const bool usb_live  = cardmic_usb_streaming();
    const bool wifi_live = s_wifi_state == WifiState::Connected && cardmic_net_receiver_active();
    const bool live      = (usb_live || wifi_live) && !s_device_muted;

    // ---- top row: link indicators left, function keys right
    // Link lights: dim = no link, lime = linked (USB plugged into a computer,
    // Wi-Fi joined), lime with a breathing ring = audio is flowing to that
    // computer right now, amber = trouble or still connecting (blinks).
    const uint32_t now = GetHAL().millis();
    auto light = [now](int x, uint32_t col, const char* label, bool flowing) {
        if (flowing) {
            float p = 0.5f + 0.5f * sinf(now * 0.0063f);  // ~1 Hz
            C().drawCircle(x + 3, 6, 5, mix(C_BG, col, 0.2f + 0.6f * p));
        }
        C().fillCircle(x + 3, 6, 3, col);
        text(&fonts::Font0, col, x + 11, 3, label);
    };
    const bool blink = (now / 350) % 2;
    uint32_t usb_col = !usb_ok ? C_WARN : cardmic_usb_mounted() ? C_ACCENT : C_DIM;
    light(2, usb_col, "USB", usb_live);
    uint32_t net_col = C_DIM;
    switch (s_wifi_state) {
        case WifiState::Connected: net_col = C_ACCENT; break;
        case WifiState::Connecting: net_col = blink ? C_WARN : C_FAINT; break;
        case WifiState::Failed: net_col = C_WARN; break;
        case WifiState::NotConfigured: net_col = C_DIM; break;
    }
    light(38, net_col, "WIFI", wifi_live);

    int kx = W - 2 - (text_w(&fonts::Font0, "S") + 5 + 3 + text_w(&fonts::Font0, "SET") + 7) -
             (text_w(&fonts::Font0, "M") + 5 + 3 + text_w(&fonts::Font0, "MUTE"));
    kx = keycap(kx, 1, "S", "SET");
    keycap(kx, 1, "M", "MUTE");

    // ---- hero
    draw_spectrogram(live, s_device_muted);

    // ---- state word + level readout
    const char* word;
    uint32_t word_col;
    if (s_talk_down && s_talk_key != TALK_OFF) {
        word = "TALK", word_col = C_CYAN;
    } else if (s_device_muted) {
        word = "MUTED", word_col = C_MUTE;
    } else if (live) {
        word = "LIVE", word_col = C_ACCENT;
    } else if (cardmic_usb_mounted() || s_wifi_state == WifiState::Connected) {
        word = "READY", word_col = C_TEXT;
    } else {
        word = "IDLE", word_col = C_DIM;
    }
    text(&fonts::Orbitron_Light_24, word_col, 2, 84, word);

    draw_meter(W - 2 - 16 * 5 + 1, 85, live, s_device_muted);

    if (s_talk_key != TALK_OFF) {
        // Hold-to-talk key cap: lit while held, dim when there is no USB keyboard link.
        const bool ready = cardmic_usb_keyboard_ready() || s_talk_down;
        const int x = 112, y = 97;
        int kw = text_w(&fonts::Font0, "SPC") + 5;
        uint32_t cap = s_talk_down ? C_CYAN : ready ? C_TEXT : C_FAINT;
        C().fillRoundRect(x, y, kw, 10, 1, cap);
        text(&fonts::Font0, s_talk_down || ready ? C_BG : C_DIM, x + 3, y + 1, "SPC");
        text(&fonts::Font0, s_talk_down ? C_CYAN : C_DIM, x + kw + 3, y + 1, ready ? "TALK" : "USB");
    }

    if (s_pair_required && cardmic_net_ms_since_unpaired() < 4000) {
        const char* msg = "UNPAIRED PC - SETTINGS > PAIRING";
        int w = text_w(&fonts::Font0, msg) + 6;
        C().fillRect(SPEC_X, SPEC_Y + SPEC_H - 11, w, 11, C_BG);
        text(&fonts::Font0, C_WARN, SPEC_X + 3, SPEC_Y + SPEC_H - 9, msg);
    }
}

void setting_row(int y, int idx, bool sel, const char* label, const std::string& value, uint32_t value_col)
{
    auto& c = C();
    if (sel) {
        c.fillRect(0, y, W, 14, C_ROW);
        c.fillRect(0, y, 3, 14, C_ACCENT);
    }
    char num[12];
    snprintf(num, sizeof(num), "%02d", idx + 1);
    text(&fonts::Font0, sel ? C_ACCENT : C_DIM, 7, y + 3, num);
    text(&fonts::DejaVu12, sel ? C_TEXT : 0xB8B8B8, 24, y + 1, label);
    text(&fonts::DejaVu12, value_col, W - 4, y + 1, value.c_str(), textdatum_t::top_right);
}

void draw_settings()
{
    char pos[24];
    snprintf(pos, sizeof(pos), "%02d/%02d", s_set_sel + 1, (int)SET_COUNT);
    page_title("SETTINGS", pos);

    std::string ssid = s_demo ? "Studio" : s_wifi_ssid.empty() ? "NOT SET" : s_wifi_ssid.substr(0, 12);
    uint32_t wcol    = s_wifi_state == WifiState::Connected ? C_ACCENT
                       : s_wifi_state == WifiState::NotConfigured ? C_DIM : C_WARN;
    std::string ip   = device_ip();
    struct Row {
        const char* label;
        std::string value;
        uint32_t col;
    };
    Row rows[SET_COUNT] = {
        {"Wi-Fi", ssid, wcol},
        {"Info", ip.empty() ? "--" : ip, C_DIM},
        {"Pairing", s_pair_required ? "ON" : "OFF", s_pair_required ? C_ACCENT : C_DIM},
        {"Talk key", TALK_KEYS[s_talk_key].name, s_talk_key != TALK_OFF ? C_CYAN : C_DIM},
        {"Mute", s_device_muted ? "ON" : "OFF", s_device_muted ? C_MUTE : C_DIM},
        {"Mic gain", GAIN_NAME[s_gain_idx], C_TEXT},
        {"About", std::string("v") + version(), C_DIM},
    };
    constexpr int VISIBLE = 5;
    int first             = std::clamp(s_set_sel - 2, 0, SET_COUNT - VISIBLE);
    for (int r = 0; r < VISIBLE; ++r) {
        int i = first + r;
        setting_row(15 + r * 16, i, i == s_set_sel, rows[i].label, rows[i].value, rows[i].col);
    }
    hint_row({{";.", "MOVE"}, {"ENT", "OPEN"}, {"G0", "BACK"}});
}

void kv(int y, const char* k, const std::string& v, uint32_t vcol = C_TEXT)
{
    text(&fonts::Font0, C_DIM, 2, y + 3, k);
    text(&fonts::DejaVu12, vcol, 62, y, v.c_str());
}

void draw_netinfo()
{
    page_title("INFO");
    std::string ssid = s_demo ? "Studio" : s_wifi_ssid.empty() ? "not set" : s_wifi_ssid;
    std::string ip   = device_ip();
    wifi_ap_record_t ap;
    std::string rssi = "--";
    if (s_wifi_state == WifiState::Connected && esp_wifi_sta_get_ap_info(&ap) == ESP_OK) {
        rssi = std::to_string(s_demo ? -48 : ap.rssi) + " dBm";
    }
    std::string rx = "none";
    if (cardmic_net_receiver_active()) {
        char buf[16];
        cardmic_net_receiver_ip(buf, sizeof(buf));
        rx = s_demo ? "192.168.1.20" : buf;
    }
    kv(16, "SSID", ssid.substr(0, 20));
    kv(30, "SIGNAL", rssi);
    kv(44, "DEVICE", ip.empty() ? "--" : ip, ip.empty() ? C_DIM : C_ACCENT);
    kv(58, "RECEIVER", rx, cardmic_net_receiver_active() ? C_ACCENT : C_DIM);
    kv(72, "PORT", "UDP 41234");
    kv(86, "AUDIO", s_pair_required ? "ENCRYPTED" : "OPEN (NO PAIRING)", s_pair_required ? C_ACCENT : C_WARN);
    hint_row({{"G0", "BACK"}});
}

void draw_pairing()
{
    page_title("PAIRING", s_pair_required ? "ON" : "OFF");
    char code[CARDMIC_PAIR_CODE_LEN + 3];
    cardmic_pair_format(s_demo ? "7K2M9QXB4TPA" : s_pair_code, code);  // docs screenshots get a sample code
    text(&fonts::FreeMonoBold9pt7b, C_TEXT, W / 2, 17, code, textdatum_t::top_center);
    text(&fonts::Font0, C_DIM, 2, 38, "ON YOUR COMPUTER, RUN ONCE:");
    text(&fonts::Font0, C_ACCENT, 2, 49, (std::string("cardmic pair ") + code).c_str());

    const char* status;
    uint32_t col;
    if (s_pair_busy) {
        status = "APPLYING...", col = C_TEXT;
    } else if (!s_pair_required) {
        status = "OFF: ANYONE ON THIS WI-FI CAN LISTEN", col = C_WARN;
    } else if (cardmic_net_encrypted()) {
        status = "ON: PAIRED COMPUTER CONNECTED", col = C_ACCENT;
    } else {
        status = "ON: ONLY PAIRED COMPUTERS, ENCRYPTED", col = C_ACCENT;
    }
    text(&fonts::Font0, col, 2, 66, status);
    text(&fonts::Font0, C_DIM, 2, 78, "NEW CODE UNPAIRS EVERY COMPUTER");
    hint_row({{"ENT", s_pair_required ? "TURN OFF" : "TURN ON"}, {"N", "NEW CODE"}, {"G0", "BACK"}});
}

void draw_about()
{
    page_title("ABOUT");
    text(&fonts::Orbitron_Light_24, C_TEXT, 2, 15, "Cardmic");
    const uint32_t cols[4] = {0x2F6BFF, 0x28F0FF, C_ACCENT, 0xFFD000};
    for (int i = 0; i < 4; ++i) C().fillRect(W - 42 + i * 10, 22, 8, 8, cols[i]);

    const esp_app_desc_t* app = esp_app_get_description();
    const esp_partition_t* run = esp_ota_get_running_partition();
    kv(38, "VERSION", std::string("v") + app->version, C_ACCENT);
    kv(50, "MODEL", GetHAL().isOriginalCardputer() ? "Cardputer" : "Cardputer ADV");
    kv(62, "SLOT", run ? run->label : "--");
    kv(74, "BUILT", std::string(app->date) + " " + std::string(app->time).substr(0, 5));

    // Update status line.
    const int y = 87;
    std::string latest = cardmic_ota_latest();
    switch (cardmic_ota_state()) {
        case CARDMIC_OTA_IDLE:
            if (!GetHAL().isWifiConnected()) text(&fonts::Font0, C_DIM, 2, y, "CONNECT WI-FI TO CHECK FOR UPDATES");
            break;
        case CARDMIC_OTA_CHECKING:
            text(&fonts::Font0, C_TEXT, 2, y, "CHECKING GITHUB...");
            break;
        case CARDMIC_OTA_UP_TO_DATE:
            text(&fonts::Font0, C_ACCENT, 2, y, ("UP TO DATE  V" + std::string(app->version) + " IS THE LATEST").c_str());
            break;
        case CARDMIC_OTA_AVAILABLE:
            text(&fonts::Font0, C_CYAN, 2, y, ("V" + latest + " AVAILABLE. INSTALL NOW?").c_str());
            break;
        case CARDMIC_OTA_DOWNLOADING: {
            int pct = cardmic_ota_progress();
            C().drawRect(2, y, 150, 7, C_DIM);
            C().fillRect(3, y + 1, 148 * pct / 100, 5, C_ACCENT);
            char buf[8];
            snprintf(buf, sizeof(buf), "%d%%", pct);
            text(&fonts::Font0, C_TEXT, W - 2, y, buf, textdatum_t::top_right);
            break;
        }
        case CARDMIC_OTA_DONE:
            text(&fonts::Font0, C_ACCENT, 2, y, "INSTALLED. RESTARTING...");
            break;
        case CARDMIC_OTA_FAILED:
            text(&fonts::Font0, C_WARN, 2, y, cardmic_ota_error());
            break;
    }
    switch (cardmic_ota_state()) {
        case CARDMIC_OTA_AVAILABLE:
            hint_row({{"ENT", "INSTALL"}, {"G0", "LATER"}});
            break;
        case CARDMIC_OTA_CHECKING:
        case CARDMIC_OTA_DOWNLOADING:
        case CARDMIC_OTA_DONE:
            break;
        default:
            hint_row({{"ENT", "CHECK UPDATE"}, {"G0", "BACK"}});
            break;
    }
}

void draw_wifi_list()
{
    auto& c = C();
    char n[16];
    snprintf(n, sizeof(n), "%d FOUND", (int)s_scan.size());
    page_title("WI-FI", n);

    int items = (int)s_scan.size() + 1;  // + "Other network..."
    int first = std::clamp(s_sel - 2, 0, std::max(0, items - 5));
    for (int row = 0; row < 5 && first + row < items; ++row) {
        int idx  = first + row;
        int y    = 15 + row * 16;
        bool sel = idx == s_sel;
        if (sel) {
            c.fillRect(0, y, W, 14, C_ROW);
            c.fillRect(0, y, 3, 14, C_ACCENT);
        }
        if (idx < (int)s_scan.size()) {
            text(&fonts::DejaVu12, sel ? C_TEXT : 0xB8B8B8, 8, y + 1, s_scan[idx].second.substr(0, 22).c_str());
            signal_bars(W - 16, y + 2, s_scan[idx].first, sel ? C_ACCENT : C_DIM, C_FAINT);
        } else {
            text(&fonts::DejaVu12, sel ? C_TEXT : C_DIM, 8, y + 1, "+ Other network...");
        }
    }
    hint_row({{";.", "MOVE"}, {"ENT", "SELECT"}, {"G0", "BACK"}});
}

void draw_entry(const char* heading, const char* label, bool password, bool failed)
{
    page_title(heading);
    text(&fonts::Font0, C_DIM, 2, 18, label);
    text_field(30, s_entry, password && !s_show_pass);
    if (failed) {
        text(&fonts::Font0, C_MUTE, 2, 60, "COULDN'T JOIN - CHECK PASSWORD");
    } else if (password && s_entry.empty()) {
        text(&fonts::Font0, C_DIM, 2, 60, "EMPTY = OPEN NETWORK");
    }
    if (password) {
        hint_row({{"TAB", s_show_pass ? "HIDE" : "SHOW"}, {"ENT", "JOIN"}, {"G0", "BACK"}});
    } else {
        hint_row({{"ENT", "NEXT"}, {"G0", "BACK"}});
    }
}

void draw_busy(const char* heading, const std::string& line)
{
    page_title(heading);
    spinner(W / 2, 50, 14);
    text(&fonts::Font0, C_DIM, W / 2, 74, line.c_str(), textdatum_t::top_center);
}

std::string upper(std::string s)
{
    for (auto& ch : s) ch = (char)toupper((unsigned char)ch);
    return s;
}

#ifdef CARDMIC_DEV_TOOLS
// Development builds accept key events over Wi-Fi (see cardmic_net.c), so the
// UI can be exercised without touching the device.
void dev_poll()
{
    uint8_t code;
    bool down;
    char name[4];
    while (cardmic_net_take_dev_key(&code, &down, name, sizeof(name))) {
        static char held[4];
        strlcpy(held, name, sizeof(held));
        Keyboard::KeyEvent_t e;
        e.state   = down;
        e.keyCode = (KeScanCode_t)code;
        e.keyName = held;
        handle_key(e);
    }
}
#endif

// Sends the whole 240x135 screen (keyboard bar + system bar + app canvas) as
// CMSS packets: 16-byte header then big-endian RGB565 rows. See PROTOCOL.md.
void send_screenshot(uint32_t addr, uint16_t port)
{
    auto& hal         = GetHAL();
    LGFX_Sprite& kb   = hal.canvasKeyboardBar;
    LGFX_Sprite& bar  = hal.canvasSystemBar;
    LGFX_Sprite& app  = hal.canvas;
    const int kbw     = kb.width();
    const int bar_h   = bar.height();
    const int sw      = kbw + app.width();
    const int sh      = kb.height();
    constexpr int ROWS = 2;

    struct __attribute__((packed)) Header {
        char magic[4];
        uint16_t width, height, y, rows;
        uint32_t reserved;
    };
    static uint8_t pkt[sizeof(Header) + 240 * 2 * ROWS];
    if (sw > 240) return;

    int sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_IP);
    if (sock < 0) return;
    sockaddr_in to{};
    to.sin_family      = AF_INET;
    to.sin_port        = port;
    to.sin_addr.s_addr = addr;

    for (int y = 0; y < sh; y += ROWS) {
        const int rows = std::min(ROWS, sh - y);
        Header h       = {{'C', 'M', 'S', 'S'}, (uint16_t)sw, (uint16_t)sh, (uint16_t)y, (uint16_t)rows, 0};
        memcpy(pkt, &h, sizeof(h));
        uint8_t* p = pkt + sizeof(h);
        for (int r = 0; r < rows; ++r) {
            const int yy = y + r;
            for (int x = 0; x < sw; ++x) {
                uint16_t c = x < kbw ? kb.readPixel(x, yy)
                             : yy < bar_h ? bar.readPixel(x - kbw, yy)
                                          : app.readPixel(x - kbw, yy - bar_h);
                *p++ = c >> 8;
                *p++ = c & 0xFF;
            }
        }
        const size_t len = p - pkt;
        for (int tries = 0; tries < 50; ++tries) {
            if (sendto(sock, pkt, len, 0, (sockaddr*)&to, sizeof(to)) >= 0) break;
            vTaskDelay(pdMS_TO_TICKS(2));  // Wi-Fi TX buffers full
        }
    }
    close(sock);
}

// Serves a pending screenshot request. A named page is rendered just for the
// picture, with demo network details, then the previous page comes back.
void serve_screenshot(const std::function<void()>& redraw)
{
    uint32_t addr;
    uint16_t port;
    char page[16];
    if (!cardmic_net_take_screenshot_request(&addr, &port, page, sizeof(page))) return;

    struct {
        const char* name;
        Page page;
    } const pages[] = {{"main", Page::Main},     {"settings", Page::Settings}, {"info", Page::NetInfo},
                       {"about", Page::About},   {"pairing", Page::Pairing}};
    const Page before = s_page;
    bool forced       = false;
    for (auto& p : pages) {
        if (!strcmp(page, p.name)) {
            s_page = p.page;
            forced = true;
        }
    }
    if (forced) {
        s_demo = true;
        redraw();
    }
    // The pairing page is only ever captured in demo mode, with a sample code.
    if (s_page != Page::Pairing || s_demo) send_screenshot(addr, port);
    if (forced) {
        s_demo = false;
        s_page = before;
    }
}

}  // namespace

/* ---------------------------------------------------------------- lifecycle */

AppCardmic::AppCardmic()
{
    setAppInfo().name     = "Cardmic";
    setAppInfo().userData = new AppIcon_t(image_data_cardmic_big, image_data_cardmic_small);
}

AppCardmic::~AppCardmic() { delete static_cast<AppIcon_t*>(getAppInfo().userData); }

void AppCardmic::onOpen()
{
    mclog::tagInfo(getAppInfo().name, "on open");

    // Take the I2S port and codec from the stock audio stack.
    audio::set_keyboard_sfx_enable(false);
    GetHAL().speaker.end();
    GetHAL().mic.end();

    // ADV: ES8311 codec over I2S. Original Cardputer: PDM microphone.
    const bool original = GetHAL().isOriginalCardputer();
    s_base_gain         = original ? PDM_BASE_GAIN : 1;
    bool audio_ok       = original ? pdm_mic_start() : (es8311_enable_mic() && i2s_mic_start());
    if (!audio_ok) {
        mclog::tagError(getAppInfo().name, "mic init failed");
    }

    std::string g = GetHAL().getSettings().GetString("cardmic_gain", "1");
    s_gain_idx    = std::clamp(atoi(g.c_str()), 0, 2);
    s_talk_key    = std::clamp(atoi(GetHAL().getSettings().GetString("cardmic_talkkey", "0").c_str()), 0, TALK_COUNT - 1);
    s_talk_down   = false;

    // USB: fails if the stock USB keyboard already owns TinyUSB this boot.
    s_usb_ok = cardmic_usb_start(s_talk_key != TALK_OFF) == ESP_OK;

    s_audio_ok = audio_ok;
    start_audio();
    pair_load();

    // WiFi: use the saved network (set in Settings > Wi-Fi, or the stock Set WiFi).
    s_wifi_ssid = GetHAL().getSettings().GetString("wifi_ssid", "");
    if (s_wifi_ssid.empty()) {
        s_wifi_state = WifiState::NotConfigured;
    } else {
        start_autoconnect();
    }

    if (!s_spec) {
        s_spec = new LGFX_Sprite(&GetHAL().canvas);
        s_spec->setColorDepth(16);
        if (!s_spec->createSprite(SPEC_W, SPEC_H)) {
            delete s_spec;
            s_spec = nullptr;
            mclog::tagError(getAppInfo().name, "no memory for spectrogram");
        }
    }
    if (s_spec) s_spec->fillScreen(TFT_BLACK);
    s_meter = s_peak_hold = 0.0f;
    s_col_pos = s_ring_w;
    s_nf_ready = false;

    s_page       = Page::Main;
    s_set_sel    = 0;
    s_intro_at   = GetHAL().millis();
    _key_slot_id = GetHAL().keyboard.onKeyEvent.connect([](const Keyboard::KeyEvent_t& e) { handle_key(e); });
    draw();
}

void AppCardmic::onRunning()
{
    sync_wifi_state();
#ifdef CARDMIC_DEV_TOOLS
    dev_poll();
#endif
    if (GetHAL().millis() - _last_draw_ms > 33) {
        draw();
        serve_screenshot([this] { draw(); });
        _last_draw_ms = GetHAL().millis();
    }
    if (GetHAL().homeButton.wasClicked() && !go_back()) {
        close();
    }
}

void AppCardmic::onClose()
{
    mclog::tagInfo(getAppInfo().name, "on close");
    if (_key_slot_id >= 0) {
        GetHAL().keyboard.onKeyEvent.disconnect(_key_slot_id);
        _key_slot_id = -1;
    }

    talk(false);
    stop_audio();
    cardmic_net_stop();
    cardmic_usb_stop();  // also hands USB back to the serial port
    i2s_mic_stop();
    if (s_spec) {
        s_spec->deleteSprite();
        delete s_spec;
        s_spec = nullptr;
    }

    // Give the audio stack back to the stock firmware.
    GetHAL().speaker.begin();
    audio::set_keyboard_sfx_enable(true);
}

void AppCardmic::draw()
{
    if (s_talk_down && s_page != Page::Main) talk(false);  // never leave the chord held
    C().fillScreen(C_BG);
    if (s_page == Page::Main && draw_intro()) {
        GetHAL().pushCanvas();
        return;
    }
    switch (s_page) {
        case Page::Main:
            draw_main(s_usb_ok);
            break;
        case Page::Settings:
            draw_settings();
            break;
        case Page::NetInfo:
            draw_netinfo();
            break;
        case Page::About:
            draw_about();
            break;
        case Page::Pairing:
            draw_pairing();
            break;
        case Page::Scanning:
            draw_busy("WI-FI", "SCANNING...");
            break;
        case Page::List:
            draw_wifi_list();
            break;
        case Page::ManualSsid:
            draw_entry("OTHER NETWORK", "NETWORK NAME (SSID)", false, false);
            break;
        case Page::Password:
            draw_entry(upper(s_pick_ssid.substr(0, 24)).c_str(), "PASSWORD", true, false);
            break;
        case Page::ConnectFailed:
            draw_entry(upper(s_pick_ssid.substr(0, 24)).c_str(), "PASSWORD", true, true);
            break;
        case Page::Connecting:
            draw_busy("WI-FI", "JOINING " + upper(s_pick_ssid.substr(0, 16)));
            break;
    }
    GetHAL().pushCanvas();
}
