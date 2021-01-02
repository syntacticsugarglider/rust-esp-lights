mod wifi;

use esp_idf_sys::{
    c_types::c_void, esp, esp_err_t, gpio_num_t_GPIO_NUM_21, gpio_num_t_GPIO_NUM_22,
    i2c_cmd_link_create, i2c_cmd_link_delete, i2c_config_t, i2c_config_t__bindgen_ty_1,
    i2c_config_t__bindgen_ty_1__bindgen_ty_1, i2c_driver_install, i2c_master_cmd_begin,
    i2c_master_start, i2c_master_stop, i2c_master_write_byte, i2c_mode_t_I2C_MODE_MASTER,
    i2c_param_config, vTaskDelay, vTaskDelete, xTaskCreatePinnedToCore, Error, I2C_NUM_1,
};
use std::{
    convert::TryInto,
    ffi::CString,
    io::Read,
    net::TcpStream,
    sync::atomic::{AtomicBool, AtomicPtr, Ordering},
};

fn update_strip(data: [u8; 3]) {
    unsafe {
        let cmd = i2c_cmd_link_create();
        esp!(i2c_master_start(cmd)).unwrap();
        esp!(i2c_master_write_byte(cmd, 0, true)).unwrap();
        for byte in &data {
            esp!(i2c_master_write_byte(cmd, *byte, true)).unwrap();
        }
        esp!(i2c_master_stop(cmd)).unwrap();
        esp!(i2c_master_cmd_begin(I2C_NUM_1 as i32, cmd, 10000)).unwrap();
        i2c_cmd_link_delete(cmd);
    }
}

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
    stack: wasmi::StackRecycler,
    module: wasmi::ModuleRef,
}

impl WasmStorage {
    fn exec(&mut self) {
        if let Some(wasmi::RuntimeValue::I32(data)) = self
            .module
            .invoke_export_with_stack("entry", &[], &mut wasmi::NopExternals, &mut self.stack)
            .expect("failed to execute entry")
        {
            let color: [u8; 3] = data.to_le_bytes()[..3].try_into().unwrap();
            update_strip(color);
            unsafe {
                vTaskDelay(10);
            }
        }
    }
}

pub extern "C" fn wasm_exec(_: *mut c_void) {
    let mut storage = unsafe { Box::from_raw(STORAGE.load(Ordering::SeqCst)) };
    while EXECUTING.load(Ordering::SeqCst) {
        storage.exec();
        unsafe { vTaskDelay(1) }
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
            master: i2c_config_t__bindgen_ty_1__bindgen_ty_1 { clk_speed: 100_000 },
        },
    };
    esp!(unsafe { i2c_param_config(I2C_NUM_1 as i32, &config) }).unwrap();
    esp!(unsafe { i2c_driver_install(I2C_NUM_1 as i32, config.mode, 0, 0, 0,) }).unwrap();
    let mut socket = TcpStream::connect(("192.168.4.217", 5000)).unwrap();
    println!("connected");
    let mut buf = vec![0u8; 1024];
    let mut len = [0u8; 4];
    loop {
        socket.read_exact(&mut len).unwrap();
        let len = u32::from_le_bytes(len) as usize;
        buf.reserve(len.saturating_sub(buf.capacity()));
        println!("reserved up to {}", len);
        socket.read_exact(&mut buf[..len]).unwrap();
        println!("read data");
        match buf[0] {
            0 => {
                if EXECUTING.load(Ordering::SeqCst) {
                    continue;
                }
                let data = String::from_utf8_lossy(&buf[1..len]);
                let data: Result<Vec<u8>, _> =
                    data.trim().split(',').map(|item| item.parse()).collect();
                if let Ok(data) = data {
                    if data.len() == 3 {
                        println!("writing {:?} to i2c...", data);
                        update_strip(data.as_slice().try_into().unwrap());
                        println!("done\n");
                    }
                }
            }
            1 => {
                EXECUTING.store(false, Ordering::SeqCst);
                unsafe { vTaskDelay(100) };
                let recycler = wasmi::StackRecycler::with_limits(4096, 10);
                let module = wasmi::Module::from_buffer(&buf[1..len]).unwrap();
                let instance =
                    wasmi::ModuleInstance::new(&module, &wasmi::ImportsBuilder::default())
                        .expect("failed to instantiate wasm module")
                        .assert_no_start();
                let storage = Box::new(WasmStorage {
                    stack: recycler,
                    module: instance,
                });
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
