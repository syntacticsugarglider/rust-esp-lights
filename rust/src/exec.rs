use esp_idf_sys::uxTaskGetStackHighWaterMark;
use led_strip::{Apa106, Color, OutputPin, RmtChannel};
use std::ffi::{CStr, CString};
use std::fmt::Debug;

use wasm3_sys::{
    m3_Call, m3_CallWithArgs, m3_FindFunction, m3_FreeEnvironment, m3_FreeRuntime, m3_GetMemory,
    m3_LoadModule, m3_NewEnvironment, m3_NewRuntime, m3_ParseModule, IM3Environment, IM3Function,
    IM3Runtime,
};

pub struct WasmExec {
    environment: IM3Environment,
    runtime: IM3Runtime,
    entry: IM3Function,
    handle_input: IM3Function,
    leds: Apa106,
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

impl WasmExec {
    pub fn new() -> Self {
        WasmExec {
            environment: std::ptr::null_mut(),
            runtime: std::ptr::null_mut(),
            entry: std::ptr::null_mut(),
            leds: Apa106::new(RmtChannel::_6, OutputPin::_4, crate::LED_COUNT).unwrap(),
            handle_input: std::ptr::null_mut(),
        }
    }

    pub unsafe fn init(&mut self, module: &[u8]) {
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
        let mut entry = std::ptr::null_mut();
        let name = CString::new("entry").unwrap();
        ckm3(m3_FindFunction(&mut entry, runtime, name.as_ptr()));
        let mut handle_input = std::ptr::null_mut();
        let name = CString::new("handle_input").unwrap();
        ckm3(m3_FindFunction(&mut handle_input, runtime, name.as_ptr()));
        self.environment = environment;
        self.entry = entry;
        self.runtime = runtime;
        self.handle_input = handle_input;
        println!(
            "wasm3 initialized, {} words left in stack at high water",
            uxTaskGetStackHighWaterMark(std::ptr::null_mut())
        );
    }

    pub unsafe fn exec(&mut self) {
        ckm3(m3_Call(self.entry));
        let ret = *((*self.runtime).stack as *const u32);
        let mem = m3_GetMemory(self.runtime, std::ptr::null_mut(), 0);
        let output = *(mem.add(ret as usize) as *const Output);
        let mut buf = [[0u8; 3]; crate::LED_COUNT];
        if output.buffered {
            let (start, end, pointer) = output.data.buffered;
            let target = &mut buf[..(end - start + 1) as usize];
            target.copy_from_slice(std::slice::from_raw_parts(
                mem.add(pointer as usize) as *const _,
                (end - start + 1) as usize,
            ));
            for (led, color) in (&mut self.leds[start as usize..]).into_iter().zip(target) {
                *led = Color {
                    red: color[0],
                    green: color[1],
                    blue: color[2],
                }
            }
        } else {
            let (start, end, color) = output.data.unbuffered;
            for led in &mut self.leds[start as usize..=end as usize] {
                *led = Color {
                    red: color[0],
                    green: color[1],
                    blue: color[2],
                }
            }
        };
        self.leds.flush().unwrap();
    }

    pub unsafe fn write(&mut self, data: &[u8]) {
        let len = CString::new(format!("{}", data.len())).unwrap();
        let argv = [len.as_c_str().as_ptr(), std::ptr::null()];
        ckm3(m3_CallWithArgs(self.handle_input, 1, argv.as_ptr()));
        let ret = *((*self.runtime).stack as *const u32);
        if ret == 0 {
            println!("program rejected input");
            return;
        }
        let mem = m3_GetMemory(self.runtime, std::ptr::null_mut(), 0);
        let output = std::slice::from_raw_parts_mut(mem.add(ret as usize), data.len());
        output.copy_from_slice(data);
        println!("wrote {} bytes to program memory: {:?}", data.len(), data);
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
