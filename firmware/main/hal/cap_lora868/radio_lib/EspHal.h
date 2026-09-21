#ifndef ESP_HAL_H
#define ESP_HAL_H

// include RadioLib
#include <RadioLib.h>

// include all the dependencies
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "esp_timer.h"
#include "esp_log.h"

// define Arduino-style macros
#define LOW     (0x0)
#define HIGH    (0x1)
#define INPUT   (0x01)
#define OUTPUT  (0x03)
#define RISING  (0x01)
#define FALLING (0x02)
#define NOP()   asm volatile("nop")

// create a new ESP-IDF hardware abstraction layer
// the HAL must inherit from the base RadioLibHal class
// and implement all of its virtual methods
class EspHal : public RadioLibHal {
public:
    // default constructor - initializes the base HAL and any needed private members
    EspHal(int8_t sck, int8_t miso, int8_t mosi, spi_host_device_t host = SPI2_HOST, bool passBusInit = false)
        : RadioLibHal(INPUT, OUTPUT, LOW, HIGH, RISING, FALLING),
          spiSCK(sck),
          spiMISO(miso),
          spiMOSI(mosi),
          spiHost(host),
          spiDevice(nullptr),
          passBusInit(passBusInit)
    {
    }

    void init() override
    {
        // initialize the SPI bus and device
        spiBegin();
    }

    void term() override
    {
        // cleanup SPI bus and device
        spiEnd();
    }

    void pinMode(uint32_t pin, uint32_t mode) override
    {
        if (pin == RADIOLIB_NC) {
            return;
        }

        gpio_config_t conf = {
            .pin_bit_mask = (1ULL << pin),
            .mode         = (gpio_mode_t)mode,
            .pull_up_en   = GPIO_PULLUP_DISABLE,
            .pull_down_en = GPIO_PULLDOWN_DISABLE,
            .intr_type    = GPIO_INTR_DISABLE,
        };
        gpio_config(&conf);
    }

    void digitalWrite(uint32_t pin, uint32_t value) override
    {
        if (pin == RADIOLIB_NC) {
            return;
        }

        gpio_set_level((gpio_num_t)pin, value);
    }

    uint32_t digitalRead(uint32_t pin) override
    {
        if (pin == RADIOLIB_NC) {
            return (0);
        }

        return (gpio_get_level((gpio_num_t)pin));
    }

    void attachInterrupt(uint32_t interruptNum, void (*interruptCb)(void), uint32_t mode) override
    {
        if (interruptNum == RADIOLIB_NC) {
            return;
        }

        // gpio_install_isr_service((int)ESP_INTR_FLAG_IRAM);
        gpio_set_intr_type((gpio_num_t)interruptNum, (gpio_int_type_t)(mode & 0x7));

        // this uses function typecasting, which is not defined when the functions have different signatures
        // untested and might not work
        gpio_isr_handler_add((gpio_num_t)interruptNum, (void (*)(void*))interruptCb, NULL);
    }

    void detachInterrupt(uint32_t interruptNum) override
    {
        if (interruptNum == RADIOLIB_NC) {
            return;
        }

        gpio_isr_handler_remove((gpio_num_t)interruptNum);
        gpio_wakeup_disable((gpio_num_t)interruptNum);
        gpio_set_intr_type((gpio_num_t)interruptNum, GPIO_INTR_DISABLE);
    }

    void delay(unsigned long ms) override
    {
        vTaskDelay(ms / portTICK_PERIOD_MS);
    }

    void delayMicroseconds(unsigned long us) override
    {
        uint64_t m = (uint64_t)esp_timer_get_time();
        if (us) {
            uint64_t e = (m + us);
            if (m > e) {  // overflow
                while ((uint64_t)esp_timer_get_time() > e) {
                    NOP();
                }
            }
            while ((uint64_t)esp_timer_get_time() < e) {
                NOP();
            }
        }
    }

    unsigned long millis() override
    {
        return ((unsigned long)(esp_timer_get_time() / 1000ULL));
    }

    unsigned long micros() override
    {
        return ((unsigned long)(esp_timer_get_time()));
    }

    long pulseIn(uint32_t pin, uint32_t state, unsigned long timeout) override
    {
        if (pin == RADIOLIB_NC) {
            return (0);
        }

        this->pinMode(pin, INPUT);
        uint32_t start   = this->micros();
        uint32_t curtick = this->micros();

        while (this->digitalRead(pin) == state) {
            if ((this->micros() - curtick) > timeout) {
                return (0);
            }
        }

        return (this->micros() - start);
    }

    void spiBegin()
    {
        esp_err_t ret;

        if (!passBusInit) {
            // configure SPI bus
            spi_bus_config_t buscfg = {
                .mosi_io_num     = this->spiMOSI,
                .miso_io_num     = this->spiMISO,
                .sclk_io_num     = this->spiSCK,
                .quadwp_io_num   = -1,
                .quadhd_io_num   = -1,
                .max_transfer_sz = 4096,
            };

            // initialize SPI bus
            ret = spi_bus_initialize(this->spiHost, &buscfg, SPI_DMA_CH_AUTO);
            if (ret != ESP_OK && ret != ESP_ERR_INVALID_STATE) {
                // ESP_ERR_INVALID_STATE means bus is already initialized
                ESP_LOGE("EspHal", "Failed to initialize SPI bus: %s", esp_err_to_name(ret));
                return;
            }
        }

        // configure SPI device
        spi_device_interface_config_t devcfg{};
        devcfg.clock_speed_hz = 2000000;  // 2 MHz
        devcfg.mode           = 0;        // SPI mode 0
        devcfg.spics_io_num   = -1;       // no CS pin managed by driver
        devcfg.queue_size     = 1;
        devcfg.flags          = 0;

        // add device to bus
        ret = spi_bus_add_device(this->spiHost, &devcfg, &this->spiDevice);
        if (ret != ESP_OK) {
            ESP_LOGE("EspHal", "Failed to add SPI device: %s", esp_err_to_name(ret));
        }
    }

    void spiBeginTransaction()
    {
        // not needed with ESP-IDF SPI driver
    }

    uint8_t spiTransferByte(uint8_t b)
    {
        if (!this->spiDevice) {
            return 0;
        }

        spi_transaction_t t;
        memset(&t, 0, sizeof(t));
        t.length    = 8;
        t.tx_buffer = &b;
        t.rx_buffer = &b;

        esp_err_t ret = spi_device_polling_transmit(this->spiDevice, &t);
        if (ret != ESP_OK) {
            ESP_LOGE("EspHal", "SPI transfer failed: %s", esp_err_to_name(ret));
            return 0;
        }

        return b;
    }

    void spiTransfer(uint8_t* out, size_t len, uint8_t* in)
    {
        if (!this->spiDevice || len == 0) {
            return;
        }

        spi_transaction_t t;
        memset(&t, 0, sizeof(t));
        t.length    = len * 8;
        t.tx_buffer = out;
        t.rx_buffer = in;

        esp_err_t ret = spi_device_polling_transmit(this->spiDevice, &t);
        if (ret != ESP_OK) {
            ESP_LOGE("EspHal", "SPI transfer failed: %s", esp_err_to_name(ret));
        }
    }

    void spiEndTransaction()
    {
        // not needed with ESP-IDF SPI driver
    }

    void spiEnd()
    {
        if (this->spiDevice) {
            spi_bus_remove_device(this->spiDevice);
            this->spiDevice = nullptr;
        }
        if (!passBusInit) {
            spi_bus_free(this->spiHost);
        }
    }

private:
    // SPI pin configuration
    int8_t spiSCK;
    int8_t spiMISO;
    int8_t spiMOSI;

    // SPI driver handles
    spi_host_device_t spiHost;
    spi_device_handle_t spiDevice;
    bool passBusInit;
};

#endif