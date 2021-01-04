mod wifi;

use esp_idf_sys::{
    c_types::c_void, esp, esp_err_t, ets_delay_us, gpio_num_t_GPIO_NUM_21, gpio_num_t_GPIO_NUM_22,
    i2c_config_t, i2c_config_t__bindgen_ty_1, i2c_config_t__bindgen_ty_1__bindgen_ty_1,
    i2c_driver_install, i2c_mode_t_I2C_MODE_MASTER, i2c_param_config, vTaskDelay, vTaskDelete,
    xTaskCreatePinnedToCore, Error, I2C_NUM_1,
};
use std::{
    convert::TryInto,
    ffi::CString,
    io::Read,
    net::TcpStream,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

mod exec;
use exec::{update_strip, Update, WasmExec};

#[no_mangle]
pub extern "C" fn main() {
    let string = CString::new("entry").unwrap();
    unsafe {
        xTaskCreatePinnedToCore(
            Some(entry),
            string.as_ptr(),
            102400,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            0,
        )
    };
}

static EXECUTING: AtomicBool = AtomicBool::new(false);
static STORAGE: AtomicPtr<WasmStorage> = AtomicPtr::new(std::ptr::null_mut());

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
    let mut storage = unsafe { Box::from_raw(STORAGE.load(Ordering::SeqCst)) };
    while EXECUTING.load(Ordering::SeqCst) {
        storage.exec();
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
    let mut buf = [0u8; 65536];
    let mut len = [0u8; 4];
    loop {
        socket.read_exact(&mut len).unwrap();
        let len = u32::from_le_bytes(len) as usize;
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
                unsafe { update_strip(Update::Unbuffered(0, 75, data.try_into().unwrap())) };
                println!("done\n");
            }
            1 => {
                EXECUTING.store(false, Ordering::SeqCst);
                unsafe { vTaskDelay(100) };
                let storage = Box::new(WasmStorage {
                    exec: WasmExec::new(&buf[1..len]),
                });
                drop(buf);
                STORAGE.store(Box::into_raw(storage), Ordering::SeqCst);
                EXECUTING.store(true, Ordering::SeqCst);
                let string = CString::new("wasm_exec").unwrap();
                unsafe {
                    xTaskCreatePinnedToCore(
                        Some(wasm_exec),
                        string.as_ptr(),
                        4096,
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
            command => {
                println!("unknown command {}", command)
            }
        }
    }
}
