mod wifi;

use esp_idf_sys::{
    c_types::c_void, esp, esp_err_t, ets_delay_us, gpio_num_t_GPIO_NUM_21, gpio_num_t_GPIO_NUM_22,
    i2c_config_t, i2c_config_t__bindgen_ty_1, i2c_config_t__bindgen_ty_1__bindgen_ty_1,
    i2c_driver_install, i2c_mode_t_I2C_MODE_MASTER, i2c_param_config, vTaskDelay, vTaskDelete,
    xTaskCreatePinnedToCore, Error, I2C_NUM_1,
};
use led_strip::{Apa106, Color, OutputPin, RmtChannel};
use std::{
    convert::TryInto,
    ffi::CString,
    io::Read,
    net::TcpStream,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

mod exec;
use exec::WasmExec;

const LED_COUNT: usize = 88;

#[no_mangle]
pub extern "C" fn main() {
    let string = CString::new("entry").unwrap();
    unsafe {
        xTaskCreatePinnedToCore(
            Some(entry),
            string.as_ptr(),
            10240,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
        )
    };
}

static EXECUTING: AtomicBool = AtomicBool::new(false);
static mut BUF: [u8; 10240] = [0u8; 10240];
static mut LEN: usize = 0;
static STORAGE: AtomicPtr<WasmStorage> = AtomicPtr::new(std::ptr::null_mut());
static INPUT_READY: AtomicBool = AtomicBool::new(false);

struct WasmStorage {
    exec: WasmExec,
}

impl WasmStorage {
    fn exec(&mut self) {
        unsafe {
            self.exec.exec();
            ets_delay_us(10_000)
        };
    }
}

pub extern "C" fn wasm_exec(_: *mut c_void) {
    EXECUTING.store(true, Ordering::SeqCst);
    println!("entered execution thread");
    let mut storage = unsafe { Box::from_raw(STORAGE.load(Ordering::SeqCst)) };
    unsafe {
        storage.exec.init(&BUF[1..LEN]);
    }
    while EXECUTING.load(Ordering::SeqCst) {
        storage.exec();
        if INPUT_READY.load(Ordering::SeqCst) {
            unsafe { storage.exec.write(&BUF[1..LEN]) };
            INPUT_READY.store(false, Ordering::SeqCst);
        }
    }
    drop(storage);
    unsafe { vTaskDelete(std::ptr::null_mut()) };
}

pub extern "C" fn entry(_: *mut c_void) {
    wifi::connect(std::env!("ESP_SSID"), std::env!("ESP_PSK")).unwrap();
    unsafe { vTaskDelay(500) };
    let config = i2c_config_t {
        mode: i2c_mode_t_I2C_MODE_MASTER,
        sda_pullup_en: true,
        scl_pullup_en: true,
        sda_io_num: gpio_num_t_GPIO_NUM_21,
        scl_io_num: gpio_num_t_GPIO_NUM_22,
        __bindgen_anon_1: i2c_config_t__bindgen_ty_1 {
            master: i2c_config_t__bindgen_ty_1__bindgen_ty_1 { clk_speed: 400_000 },
        },
    };
    esp!(unsafe { i2c_param_config(I2C_NUM_1 as i32, &config) }).unwrap();
    esp!(unsafe { i2c_driver_install(I2C_NUM_1 as i32, config.mode, 0, 0, 0,) }).unwrap();
    let mut socket = TcpStream::connect(("192.168.4.250", 5000)).unwrap();
    println!("connected");
    let buf = unsafe { &mut BUF };
    let mut len = [0u8; 4];
    loop {
        socket.read_exact(&mut len).unwrap();
        let len = u32::from_le_bytes(len) as usize;
        INPUT_READY.store(false, Ordering::SeqCst);
        socket.read_exact(&mut buf[..len]).unwrap();
        println!("read {}", len);
        match buf[0] {
            0 => {
                if EXECUTING.load(Ordering::SeqCst) {
                    EXECUTING.store(false, Ordering::SeqCst);
                    unsafe { vTaskDelay(100) }
                }
                let data = &buf[1..len];
                println!("writing {:?} to i2c...", data);
                let mut leds = Apa106::new(RmtChannel::_6, OutputPin::_4, LED_COUNT).unwrap();
                let data: [u8; 3] = data.try_into().unwrap();
                for led in &mut leds {
                    *led = Color {
                        red: data[0],
                        green: data[1],
                        blue: data[2],
                    }
                }
                leds.flush().unwrap();
                println!("done\n");
            }
            1 => {
                EXECUTING.store(false, Ordering::SeqCst);
                unsafe { vTaskDelay(100) };
                unsafe {
                    LEN = len;
                }
                let storage = Box::new(WasmStorage {
                    exec: WasmExec::new(),
                });
                STORAGE.store(Box::into_raw(storage), Ordering::SeqCst);
                let string = CString::new("wasm_exec").unwrap();
                unsafe {
                    xTaskCreatePinnedToCore(
                        Some(wasm_exec),
                        string.as_ptr(),
                        65535,
                        std::ptr::null_mut(),
                        0,
                        std::ptr::null_mut(),
                        1,
                    )
                };
            }
            2 => {
                EXECUTING.store(false, Ordering::SeqCst);
                unsafe { vTaskDelay(100) }
            }
            3 => {
                if EXECUTING.load(Ordering::SeqCst) {
                    println!("got {} bytes for program input", len);
                    unsafe {
                        LEN = len;
                    }
                    INPUT_READY.store(true, Ordering::SeqCst);
                }
            }
            command => {
                println!("unknown command {}", command)
            }
        }
    }
}
