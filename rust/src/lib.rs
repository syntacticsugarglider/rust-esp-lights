mod wifi;

use esp_idf_sys::vTaskDelay;
use led_strip::{Apa106, Color, OutputPin, RmtChannel};
use std::{io::Read, net::TcpStream};

#[no_mangle]
pub extern "C" fn main() {
    let mut strip = Apa106::new(RmtChannel::_6, OutputPin::_4, 33).unwrap();
    wifi::connect(std::env!("ESP_SSID"), std::env!("ESP_PSK")).unwrap();
    unsafe { vTaskDelay(500) };
    let mut socket = TcpStream::connect(("192.168.4.250", 5000)).unwrap();
    let mut buf = vec![0u8; 1024];
    loop {
        let data = socket.read(&mut buf).unwrap();
        let data = &buf[..data];
        let data = String::from_utf8_lossy(data);
        let data: Result<Vec<u8>, _> = data.trim().split(',').map(|item| item.parse()).collect();
        if let Ok(data) = data {
            if data.len() == 3 {
                let color = Color {
                    red: data[0],
                    green: data[1],
                    blue: data[2],
                };
                for led in &mut strip {
                    *led = color;
                }
                strip.flush().unwrap();
            }
        }
    }
}
