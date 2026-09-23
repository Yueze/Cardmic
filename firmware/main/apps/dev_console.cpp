/*
 * Cardmic development console (CARDMIC_DEV_TOOLS builds only, never in a
 * release). A line-based command channel on the USB serial/JTAG port, so the
 * device can be driven and inspected without pressing its keys and without
 * Wi-Fi. It is available whenever the stock launcher owns USB -- inside
 * Cardmic, TinyUSB owns USB and the Wi-Fi commands in cardmic_net.c take over.
 *
 *   key <row> <col> <0|1>   key down/up, as if typed (launcher: ; = 2 11,
 *                           . = 3 11, enter = 2 13)
 *   tap <row> <col>         down then up
 *   shot                    whole screen as base64 RGB565, between SHOT/END
 *   wifi <ssid> <password>  store network credentials
 *   status                  board, keyboard, Wi-Fi, IP, free heap
 *   reboot | download       restart, or restart into the ROM bootloader
 *
 * SPDX-License-Identifier: MIT
 */
#ifdef CARDMIC_DEV_TOOLS

#include <cstdio>
#include <cstring>
#include <string>

#include "driver/usb_serial_jtag.h"
#include "esp_heap_caps.h"
#include "esp_netif.h"
#include "esp_system.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "hal.h"
#include "dev_console.h"
#include "soc/rtc_cntl_reg.h"

namespace {

// Where replies go for the channel currently running a command: the
// serial/JTAG port in the launcher, or the app's USB serial port.
thread_local cardmic_dev_reply_t g_reply = nullptr;

void reply(const std::string& line)
{
    if (g_reply) g_reply(line.c_str());
}

void reply_serial_jtag(const char* line)
{
    std::string out = std::string(line) + "\r\n";
    usb_serial_jtag_write_bytes(out.data(), out.size(), pdMS_TO_TICKS(500));
}

void send_screen()
{
    auto& hal        = GetHAL();
    LGFX_Sprite& kb  = hal.canvasKeyboardBar;
    LGFX_Sprite& bar = hal.canvasSystemBar;
    LGFX_Sprite& app = hal.canvas;
    const int kbw = kb.width(), bar_h = bar.height();
    const int w = kbw + app.width(), h = kb.height();
    reply("SHOT " + std::to_string(w) + " " + std::to_string(h));

    static const char* B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    std::string line;
    uint32_t acc = 0;
    int bits     = 0;
    auto put     = [&](uint8_t byte) {
        acc = (acc << 8) | byte;
        bits += 8;
        while (bits >= 6) {
            bits -= 6;
            line += B64[(acc >> bits) & 0x3F];
        }
        if (line.size() >= 180) {
            reply(line);
            line.clear();
        }
    };
    for (int y = 0; y < h; ++y) {
        for (int x = 0; x < w; ++x) {
            uint16_t c = x < kbw      ? kb.readPixel(x, y)
                         : y < bar_h  ? bar.readPixel(x - kbw, y)
                                      : app.readPixel(x - kbw, y - bar_h);
            put(c >> 8);
            put(c & 0xFF);
        }
    }
    if (bits > 0) line += B64[(acc << (6 - bits)) & 0x3F];
    if (!line.empty()) reply(line);
    reply("END");
}

void run_command(const std::string& cmd)
{
    char a[64] = "", b[64] = "";
    int row = 0, col = 0, state = 0;

    if (sscanf(cmd.c_str(), "key %d %d %d", &row, &col, &state) == 3) {
        GetHAL().keyboard.injectKey((uint8_t)row, (uint8_t)col, state != 0);
        reply("OK");
    } else if (sscanf(cmd.c_str(), "tap %d %d", &row, &col) == 2) {
        GetHAL().keyboard.injectKey((uint8_t)row, (uint8_t)col, true);
        vTaskDelay(pdMS_TO_TICKS(60));
        GetHAL().keyboard.injectKey((uint8_t)row, (uint8_t)col, false);
        reply("OK");
    } else if (cmd == "shot") {
        send_screen();
    } else if (sscanf(cmd.c_str(), "wifi %63s %63s", a, b) == 2) {
        GetHAL().getSettings().SetString("wifi_ssid", a);
        GetHAL().getSettings().SetString("wifi_password", b);
        reply(std::string("OK saved ") + a);
    } else if (cmd == "scan") {
        GetHAL().wifiInit();
        std::vector<Hal::ScanResult_t> found;
        GetHAL().wifiScan(found);
        for (auto& r : found) reply(std::to_string(r.first) + " dBm  " + r.second);
        reply("OK " + std::to_string(found.size()) + " networks");
    } else if (sscanf(cmd.c_str(), "join %63s %63s", a, b) >= 1) {
        // Test a join from the launcher and report the plain result. With one
        // argument the stored password is used.
        std::string pass = (sscanf(cmd.c_str(), "join %63s %63s", a, b) == 2)
                               ? b
                               : GetHAL().getSettings().GetString("wifi_password", "");
        bool ok = GetHAL().wifiConnect(a, pass);
        reply(ok ? "OK joined" : "ERR join failed");
    } else if (cmd == "status") {
        std::string ip = "none";
        esp_netif_t* nif = esp_netif_get_handle_from_ifkey("WIFI_STA_DEF");
        esp_netif_ip_info_t info;
        if (nif && esp_netif_get_ip_info(nif, &info) == ESP_OK && info.ip.addr) {
            char buf[16];
            snprintf(buf, sizeof(buf), IPSTR, IP2STR(&info.ip));
            ip = buf;
        }
        reply(std::string("board=") + (GetHAL().isOriginalCardputer() ? "Cardputer" : "CardputerADV") +
              " keyboard=" + (GetHAL().isKeyboardReady() ? "ok" : "missing") +
              " wifi=" + (GetHAL().isWifiConnected() ? "up" : "down") + " ip=" + ip +
              " ssid=" + GetHAL().getSettings().GetString("wifi_ssid", "") +
              " password=" + std::to_string(GetHAL().getSettings().GetString("wifi_password", "").size()) + "chars" +
              " heap=" + std::to_string(heap_caps_get_free_size(MALLOC_CAP_INTERNAL)));
    } else if (cmd == "reboot") {
        reply("OK");
        vTaskDelay(pdMS_TO_TICKS(100));
        esp_restart();
    } else if (cmd == "download") {
        reply("OK");
        vTaskDelay(pdMS_TO_TICKS(100));
        REG_WRITE(RTC_CNTL_OPTION1_REG, RTC_CNTL_FORCE_DOWNLOAD_BOOT);
        esp_restart();
    } else if (!cmd.empty()) {
        reply("ERR unknown command");
    }
}

void console_task(void*)
{
    g_reply = reply_serial_jtag;
    std::string line;
    uint8_t ch;
    while (true) {
        if (usb_serial_jtag_read_bytes(&ch, 1, portMAX_DELAY) != 1) {
            continue;
        }
        if (ch == '\n' || ch == '\r') {
            if (!line.empty()) {
                run_command(line);
                line.clear();
            }
        } else if (line.size() < 200) {
            line += (char)ch;
        }
    }
}

}  // namespace

// Runs one command line on behalf of another channel (the app's USB serial
// port); replies go to the supplied callback.
void cardmic_dev_command(const char* line, cardmic_dev_reply_t reply_to)
{
    cardmic_dev_reply_t previous = g_reply;
    g_reply                      = reply_to;
    run_command(line);
    g_reply = previous;
}

void cardmic_dev_console_start()
{
    usb_serial_jtag_driver_config_t cfg = USB_SERIAL_JTAG_DRIVER_CONFIG_DEFAULT();
    cfg.rx_buffer_size = 1024;
    cfg.tx_buffer_size = 2048;
    if (usb_serial_jtag_driver_install(&cfg) != ESP_OK) {
        return;
    }
    xTaskCreate(console_task, "cardmic_devcon", 6144, nullptr, 3, nullptr);
}

#endif  // CARDMIC_DEV_TOOLS
