/* UART Echo Example

   This example code is in the Public Domain (or CC0 licensed, at your option.)

   Unless required by applicable law or agreed to in writing, this
   software is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
   CONDITIONS OF ANY KIND, either express or implied.
*/
#include "uart_helper.h"
#include "../../hal_config.h"
#include <stdio.h>
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "driver/uart.h"
#include "driver/gpio.h"
#include "sdkconfig.h"
#include "esp_log.h"

/**
 * This is an example which echos any data it receives on configured UART back to the sender,
 * with hardware flow control turned off. It does not use UART driver event queue.
 *
 * - Port: configured UART
 * - Receive (Rx) buffer: on
 * - Transmit (Tx) buffer: off
 * - Flow control: off
 * - Event queue: off
 * - Pin assignment: see defines below (See Kconfig)
 */

#define ECHO_TEST_TXD (HAL_PIN_GPS_TX)
#define ECHO_TEST_RXD (HAL_PIN_GPS_RX)
#define ECHO_TEST_RTS (UART_PIN_NO_CHANGE)
#define ECHO_TEST_CTS (UART_PIN_NO_CHANGE)

#define ECHO_UART_PORT_NUM   (2)
#define ECHO_UART_BAUD_RATE  (115200)
#define ECHO_TASK_STACK_SIZE (4096)

static const char *TAG = "UartHelper";

#define BUF_SIZE (512)

static gps_uart_helper_on_msg_callback_t _on_msg = NULL;

static void echo_task(void *arg)
{
    ESP_LOGI(TAG, "gps uart helper task start");

    gpio_reset_pin(ECHO_TEST_TXD);
    gpio_reset_pin(ECHO_TEST_RXD);

    /* Configure parameters of an UART driver,
     * communication pins and install the driver */
    uart_config_t uart_config = {
        .baud_rate  = ECHO_UART_BAUD_RATE,
        .data_bits  = UART_DATA_8_BITS,
        .parity     = UART_PARITY_DISABLE,
        .stop_bits  = UART_STOP_BITS_1,
        .flow_ctrl  = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
    };
    int intr_alloc_flags = 0;

#if CONFIG_UART_ISR_IN_IRAM
    intr_alloc_flags = ESP_INTR_FLAG_IRAM;
#endif

    ESP_ERROR_CHECK(uart_driver_install(ECHO_UART_PORT_NUM, BUF_SIZE * 2, 0, 0, NULL, intr_alloc_flags));
    ESP_ERROR_CHECK(uart_param_config(ECHO_UART_PORT_NUM, &uart_config));
    ESP_ERROR_CHECK(uart_set_pin(ECHO_UART_PORT_NUM, ECHO_TEST_TXD, ECHO_TEST_RXD, ECHO_TEST_RTS, ECHO_TEST_CTS));

    // Configure a temporary buffer for the incoming data
    uint8_t *data = (uint8_t *)malloc(BUF_SIZE);

    while (1) {
        // Read data from the UART
        int len = uart_read_bytes(ECHO_UART_PORT_NUM, data, (BUF_SIZE - 1), portMAX_DELAY);
        // Write data back to the UART
        //        uart_write_bytes(ECHO_UART_PORT_NUM, (const char *) data, len);
        if (len) {
            data[len] = '\0';
            // ESP_LOGI(TAG, "Recv str: %s", (char *)data);

            if (_on_msg) {
                _on_msg((const char *)data);
            }
        }
    }
}

static void _demo_app_main(void)
{
    xTaskCreate(echo_task, "gps_uart", ECHO_TASK_STACK_SIZE, NULL, 10, NULL);
}

void gps_uart_helper_init()
{
    _demo_app_main();
}

void gps_uart_helper_set_on_msg_callback(gps_uart_helper_on_msg_callback_t callback)
{
    _on_msg = callback;
}
