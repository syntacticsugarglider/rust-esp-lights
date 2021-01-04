use std::ffi::CString;

use esp_idf_sys::{
    c_types, esp, esp_err_t, esp_event_base_t, esp_event_handler_register,
    esp_event_loop_create_default, esp_event_send_internal, esp_interface_t_ESP_IF_WIFI_STA,
    esp_netif_create_default_wifi_sta, esp_netif_init, esp_nofail, esp_wifi_connect, esp_wifi_init,
    esp_wifi_set_config, esp_wifi_set_mode, esp_wifi_start, g_wifi_default_wpa_crypto_funcs,
    g_wifi_osi_funcs, ip_event_got_ip_t, ip_event_t_IP_EVENT_STA_GOT_IP, nvs_flash_erase,
    nvs_flash_init, wifi_auth_mode_t_WIFI_AUTH_OPEN, wifi_config_t,
    wifi_event_t_WIFI_EVENT_STA_DISCONNECTED, wifi_event_t_WIFI_EVENT_STA_START,
    wifi_init_config_t, wifi_mode_t_WIFI_MODE_STA, wifi_pmf_config_t,
    wifi_scan_method_t_WIFI_FAST_SCAN, wifi_scan_threshold_t,
    wifi_sort_method_t_WIFI_CONNECT_AP_BY_SIGNAL, wifi_sta_config_t, xTaskGetCurrentTaskHandle,
    Error, ESP_ERR_NVS_NEW_VERSION_FOUND, ESP_ERR_NVS_NO_FREE_PAGES, ESP_EVENT_ANY_ID, IP_EVENT,
    WIFI_EVENT,
};

pub fn connect<T: AsRef<str>, U: AsRef<str>>(ssid: T, pass: U) -> Result<(), Error> {
    unsafe {
        esp!(esp_netif_init())?;
        esp!(esp_event_loop_create_default())?;

        if let Some(err) = Error::from(nvs_flash_init()) {
            match err.code() as u32 {
                ESP_ERR_NVS_NO_FREE_PAGES | ESP_ERR_NVS_NEW_VERSION_FOUND => {
                    esp!(nvs_flash_erase())?
                }
                _ => (),
            }
        }

        esp!(nvs_flash_init())?;

        let cfg = wifi_init_config_t {
            event_handler: Some(esp_event_send_internal),
            osi_funcs: &mut g_wifi_osi_funcs,
            wpa_crypto_funcs: g_wifi_default_wpa_crypto_funcs,
            static_rx_buf_num: 10,
            dynamic_rx_buf_num: 32,
            tx_buf_type: 1,
            static_tx_buf_num: 0,
            dynamic_tx_buf_num: 32,
            csi_enable: 0,
            ampdu_rx_enable: 1,
            ampdu_tx_enable: 1,
            nvs_enable: 0,
            nano_enable: 0,
            tx_ba_win: 6,
            rx_ba_win: 6,
            wifi_task_core_id: 0,
            beacon_max_len: 752,
            mgmt_sbuf_num: 32,
            feature_caps: 1,
            magic: 0x1F2F3F4F,
        };
        esp!(esp_wifi_init(&cfg))?;

        let task = xTaskGetCurrentTaskHandle();

        esp!(esp_event_handler_register(
            WIFI_EVENT,
            ESP_EVENT_ANY_ID,
            Option::Some(event_handler),
            task
        ))?;
        esp!(esp_event_handler_register(
            IP_EVENT,
            ip_event_t_IP_EVENT_STA_GOT_IP as i32,
            Option::Some(event_handler),
            task
        ))?;

        // Initialize default station as network interface instance (esp-netif)
        let _esp_netif_t = esp_netif_create_default_wifi_sta();

        // Initialize and start WiFi
        let mut wifi_config = wifi_config_t {
            sta: wifi_sta_config_t {
                ssid: [0; 32],
                password: [0; 64],
                scan_method: wifi_scan_method_t_WIFI_FAST_SCAN,
                bssid_set: false,
                bssid: [0; 6],
                channel: 0,
                listen_interval: 0,
                sort_method: wifi_sort_method_t_WIFI_CONNECT_AP_BY_SIGNAL,
                threshold: wifi_scan_threshold_t {
                    rssi: 127,
                    authmode: wifi_auth_mode_t_WIFI_AUTH_OPEN,
                },
                pmf_cfg: wifi_pmf_config_t {
                    capable: false,
                    required: false,
                },
            },
        };

        set_str(&mut wifi_config.sta.ssid, ssid.as_ref());
        set_str(&mut wifi_config.sta.password, pass.as_ref());

        esp!(esp_wifi_set_mode(wifi_mode_t_WIFI_MODE_STA))?;
        esp!(esp_wifi_set_config(
            esp_interface_t_ESP_IF_WIFI_STA,
            &mut wifi_config
        ))?;
        esp!(esp_wifi_start())?;

        Ok(())
    }
}

unsafe extern "C" fn event_handler(
    _arg: *mut c_types::c_void,
    event_base: esp_event_base_t,
    event_id: c_types::c_int,
    event_data: *mut c_types::c_void,
) {
    if event_base == WIFI_EVENT && event_id == wifi_event_t_WIFI_EVENT_STA_START as i32 {
        esp_nofail!(esp_wifi_connect());
    } else if event_base == WIFI_EVENT
        && event_id == wifi_event_t_WIFI_EVENT_STA_DISCONNECTED as i32
    {
        esp_nofail!(esp_wifi_connect());
    } else if event_base == IP_EVENT && event_id == ip_event_t_IP_EVENT_STA_GOT_IP as i32 {
        let event: *const ip_event_got_ip_t = std::mem::transmute(event_data);
        println!("NETIF: Got IP: {:?}", (*event).ip_info);
    }
}

fn set_str(buf: &mut [u8], s: &str) {
    let cs = CString::new(s).unwrap();
    let ss: &[u8] = cs.as_bytes_with_nul();
    buf[..ss.len()].copy_from_slice(&ss);
}
