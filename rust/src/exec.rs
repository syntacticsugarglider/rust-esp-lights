use esp_idf_sys::{
    esp, esp_err_t, i2c_cmd_link_create, i2c_cmd_link_delete, i2c_master_cmd_begin,
    i2c_master_start, i2c_master_stop, i2c_master_write_byte, Error, I2C_NUM_1,
};
use std::ffi::{CStr, CString};
use std::fmt::Debug;

use wasm3_sys::{
    m3_Call, m3_FindFunction, m3_FreeEnvironment, m3_FreeRuntime, m3_GetMemory, m3_LoadModule,
    m3_NewEnvironment, m3_NewRuntime, m3_ParseModule, IM3Environment, IM3Function, IM3Runtime,
};

pub unsafe fn update_strip(data: Update<'_>) {
    let cmd = i2c_cmd_link_create();
    esp!(i2c_master_start(cmd)).unwrap();
    match data {
        Update::Unbuffered(start, finish, color) => {
            esp!(i2c_master_write_byte(cmd, 0, true)).unwrap();
            esp!(i2c_master_write_byte(cmd, start, true)).unwrap();
            esp!(i2c_master_write_byte(cmd, finish, true)).unwrap();
            for byte in &color {
                esp!(i2c_master_write_byte(cmd, *byte, true)).unwrap();
            }
        }
        Update::Buffered(start, finish, buffer) => {
            esp!(i2c_master_write_byte(cmd, 0, true)).unwrap();
            esp!(i2c_master_write_byte(cmd, 75 + start, true)).unwrap();
            esp!(i2c_master_write_byte(cmd, finish, true)).unwrap();
            for byte in buffer.iter().flatten() {
                esp!(i2c_master_write_byte(cmd, *byte, true)).unwrap();
            }
        }
    }
    esp!(i2c_master_stop(cmd)).unwrap();
    esp!(i2c_master_cmd_begin(I2C_NUM_1 as i32, cmd, 10000)).unwrap();
    i2c_cmd_link_delete(cmd);
}

pub struct WasmExec {
    environment: IM3Environment,
    runtime: IM3Runtime,
    method: IM3Function,
}

#[track_caller]
unsafe fn ckm3(res: *const i8) {
    if !res.is_null() {
        panic!("{}", CStr::from_ptr(res).to_str().unwrap());
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
union OutputData {
    unbuffered: (u8, u8, [u8; 3]),
    buffered: (u8, u8, u32),
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Output {
    buffered: bool,
    data: OutputData,
}

impl Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", unsafe {
            if self.buffered {
                format!("buffered: {:?}", self.data.buffered)
            } else {
                format!("unbuffered: {:?}", self.data.unbuffered)
            }
        })
    }
}

#[derive(Debug)]
pub enum Update<'a> {
    Buffered(u8, u8, &'a [[u8; 3]]),
    Unbuffered(u8, u8, [u8; 3]),
}

impl WasmExec {
    pub fn new(module: &[u8]) -> Self {
        unsafe {
            let environment = m3_NewEnvironment();
            let runtime = m3_NewRuntime(environment, 8192, std::ptr::null_mut());
            if runtime.is_null() {
                panic!("constructing runtime failed");
            }
            let mut m3_module = std::ptr::null_mut();
            ckm3(m3_ParseModule(
                environment,
                &mut m3_module,
                module.as_ptr(),
                module.len() as u32,
            ));
            ckm3(m3_LoadModule(runtime, m3_module));
            let mut m3_method = std::ptr::null_mut();
            let name = CString::new("entry").unwrap();
            ckm3(m3_FindFunction(&mut m3_method, runtime, name.as_ptr()));
            WasmExec {
                environment,
                runtime,
                method: m3_method,
            }
        }
    }

    pub unsafe fn exec(&mut self) {
        ckm3(m3_Call(self.method));
        let ret = *((*self.runtime).stack as *const u32);
        let mem = m3_GetMemory(self.runtime, std::ptr::null_mut(), 0);
        let output = *(mem.add(ret as usize) as *const Output);
        let mut buf = [[0u8; 3]; 75];
        update_strip(if output.buffered {
            let (start, end, pointer) = output.data.buffered;
            let target = &mut buf[..(end - start + 1) as usize];
            target.copy_from_slice(std::slice::from_raw_parts(
                mem.add(pointer as usize) as *const _,
                (end - start + 1) as usize,
            ));
            Update::Buffered(start, end, &*target)
        } else {
            let (start, end, color) = output.data.unbuffered;
            Update::Unbuffered(start, end, color)
        });
    }
}

impl Drop for WasmExec {
    fn drop(&mut self) {
        unsafe {
            m3_FreeRuntime(self.runtime);
            m3_FreeEnvironment(self.environment);
        }
    }
}
