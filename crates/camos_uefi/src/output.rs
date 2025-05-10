use core::cell::UnsafeCell;

use uart_16550::SerialPort;

/// The address of the COM1 serial port
pub const COM1: u16 = 0x3F8;

pub struct SerialPortWrapper(pub UnsafeCell<SerialPort>);

impl SerialPortWrapper {
    pub const fn new(port: SerialPort) -> Self {
        Self(UnsafeCell::new(port))
    }
}

unsafe impl Send for SerialPortWrapper {}
unsafe impl Sync for SerialPortWrapper {}

/// The serial port used to write console output to. The loader only runs on
/// one core so it is fine to get mutable access to this.
pub static SERIAL: SerialPortWrapper = SerialPortWrapper::new(unsafe { SerialPort::new(COM1) });

#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        {
            use core::fmt::Write as _;
    
            core::writeln!(unsafe { &mut *$crate::output::SERIAL.0.get() }, $($arg)*).unwrap()
        }
    };
}
