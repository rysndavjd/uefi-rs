// SPDX-License-Identifier: MIT OR Apache-2.0

//! Provides support for the UEFI debugging protocol.
//!
//! This protocol is designed to allow debuggers to query the state of the firmware,
//! as well as set up callbacks for various events.
//!
//! It also defines a Debugport protocol for debugging over serial devices.
//!
//! An example UEFI debugger is Intel's [UDK Debugger Tool][udk].
//!
//! [udk]: https://firmware.intel.com/develop/intel-uefi-tools-and-utilities/intel-uefi-development-kit-debugger-tool

use core::ffi::c_void;

use crate::proto::unsafe_protocol;
use crate::{Result, Status, StatusExt};

// re-export for ease of use
pub use context::SystemContext;
pub use exception::ExceptionType;

mod context;
mod exception;

/// Debug support [`Protocol`].
///
/// The debugging support protocol allows debuggers to connect to a UEFI machine.
/// It is expected that there will typically be two instances of the EFI Debug Support protocol in the system.
/// One associated with the native processor instruction set (IA-32, x64, ARM, RISC-V, or Itanium processor
/// family), and one for the EFI virtual machine that implements EFI byte code (EBC).
/// While multiple instances of the EFI Debug Support protocol are expected, there must never be more than
/// one for any given instruction set.
///
/// NOTE: OVMF only implements this protocol interface for the virtual EBC processor
///
/// [`Protocol`]: uefi::proto::Protocol
#[derive(Debug)]
#[repr(C)]
#[unsafe_protocol("2755590c-6f3c-42fa-9ea4-a3ba543cda25")]
pub struct DebugSupport {
    isa: ProcessorArch,
    get_maximum_processor_index:
        extern "efiapi" fn(this: &mut DebugSupport, max_processor_index: &mut usize) -> Status,
    register_periodic_callback: unsafe extern "efiapi" fn(
        this: &mut DebugSupport,
        processor_index: usize,
        periodic_callback: Option<unsafe extern "efiapi" fn(SystemContext)>,
    ) -> Status,
    register_exception_callback: unsafe extern "efiapi" fn(
        this: &mut DebugSupport,
        processor_index: usize,
        exception_callback: Option<unsafe extern "efiapi" fn(ExceptionType, SystemContext)>,
        exception_type: ExceptionType,
    ) -> Status,
    invalidate_instruction_cache: unsafe extern "efiapi" fn(
        this: &mut DebugSupport,
        processor_index: usize,
        start: *mut c_void,
        length: u64,
    ) -> Status,
}

impl DebugSupport {
    /// Returns the processor architecture of the running CPU.
    #[must_use]
    pub const fn arch(&self) -> ProcessorArch {
        self.isa
    }

    /// Returns the maximum value that may be used for the processor_index parameter in
    /// `register_periodic_callback()` and `register_exception_callback()`.
    ///
    /// Note: Applications built with EDK2 (such as OVMF) always return `0` as of 2021-09-15
    pub fn get_maximum_processor_index(&mut self) -> usize {
        // initially set to a canary value for testing purposes
        let mut max_processor_index: usize = usize::MAX;

        // per the UEFI spec, this call should only return EFI_SUCCESS
        let _ = (self.get_maximum_processor_index)(self, &mut max_processor_index);

        max_processor_index
    }

    /// Registers a function to be called back periodically in interrupt context.
    /// Pass `None` for `callback` to deregister the currently registered function for
    /// a specified `processor_index`. Will return `Status::INVALID_PARAMETER` if
    /// `processor_index` exceeds the current maximum from `Self::get_maximum_processor_index`.
    ///
    /// Note: Applications built with EDK2 (such as OVMF) ignore the `processor_index` parameter
    ///
    /// # Safety
    /// No portion of the debug agent that runs in interrupt context may make any
    /// calls to EFI services or other protocol interfaces.
    pub unsafe fn register_periodic_callback(
        &mut self,
        processor_index: usize,
        callback: Option<unsafe extern "efiapi" fn(SystemContext)>,
    ) -> Result {
        if processor_index > self.get_maximum_processor_index() {
            return Err(Status::INVALID_PARAMETER.into());
        }

        // Safety: As we've validated the `processor_index`, this should always be safe
        unsafe { (self.register_periodic_callback)(self, processor_index, callback) }.to_result()
    }

    /// Registers a function to be called when a given processor exception occurs.
    /// Pass `None` for `callback` to deregister the currently registered function for a
    /// given `exception_type` and `processor_index`. Will return `Status::INVALID_PARAMETER`
    /// if `processor_index` exceeds the current maximum from `Self::get_maximum_processor_index`.
    ///
    /// Note: Applications built with EDK2 (such as OVMF) ignore the `processor_index` parameter
    ///
    /// # Safety
    /// No portion of the debug agent that runs in interrupt context may make any
    /// calls to EFI services or other protocol interfaces.
    pub unsafe fn register_exception_callback(
        &mut self,
        processor_index: usize,
        callback: Option<unsafe extern "efiapi" fn(ExceptionType, SystemContext)>,
        exception_type: ExceptionType,
    ) -> Result {
        if processor_index > self.get_maximum_processor_index() {
            return Err(Status::INVALID_PARAMETER.into());
        }

        // Safety: As we've validated the `processor_index`, this should always be safe
        unsafe {
            (self.register_exception_callback)(self, processor_index, callback, exception_type)
        }
        .to_result()
    }

    /// Invalidates processor instruction cache for a memory range for a given `processor_index`.
    ///
    /// Note: Applications built with EDK2 (such as OVMF) ignore the `processor_index` parameter
    ///
    /// # Safety
    /// `start` must be a c_void ptr to a valid memory address
    pub unsafe fn invalidate_instruction_cache(
        &mut self,
        processor_index: usize,
        start: *mut c_void,
        length: u64,
    ) -> Result {
        if processor_index > self.get_maximum_processor_index() {
            return Err(Status::INVALID_PARAMETER.into());
        }

        // per the UEFI spec, this call should only return EFI_SUCCESS
        // Safety: As we've validated the `processor_index`, this should always be safe
        unsafe { (self.invalidate_instruction_cache)(self, processor_index, start, length) }
            .to_result()
    }
}

newtype_enum! {
/// The instruction set architecture of the running processor.
///
/// UEFI can be and has been ported to new CPU architectures in the past,
/// therefore modeling this C enum as a Rust enum (where the compiler must know
/// about every variant in existence) would _not_ be safe.
pub enum ProcessorArch: u32 => {
    /// 32-bit x86 PC
    X86_32      = 0x014C,
    /// 64-bit x86 PC
    X86_64      = 0x8664,
    /// Intel Itanium
    ITANIUM     = 0x200,
    /// UEFI Interpreter bytecode
    EBC         = 0x0EBC,
    /// ARM Thumb / Mixed
    ARM         = 0x01C2,
    /// ARM 64-bit
    AARCH_64    = 0xAA64,
    /// RISC-V 32-bit
    RISCV_32    = 0x5032,
    /// RISC-V 64-bit
    RISCV_64    = 0x5064,
    /// RISC-V 128-bit
    RISCV_128   = 0x5128,
}}

/// Debug Port [`Protocol`].
///
/// The debug port protocol abstracts the underlying debug port
/// hardware, whether it is a regular Serial port or something else.
///
/// [`Protocol`]: uefi::proto::Protocol
#[derive(Debug)]
#[repr(C)]
#[unsafe_protocol("eba4e8d2-3858-41ec-a281-2647ba9660d0")]
pub struct DebugPort {
    reset: extern "efiapi" fn(this: &DebugPort) -> Status,
    write: extern "efiapi" fn(
        this: &DebugPort,
        timeout: u32,
        buffer_size: &mut usize,
        buffer: *const c_void,
    ) -> Status,
    read: extern "efiapi" fn(
        this: &DebugPort,
        timeout: u32,
        buffer_size: &mut usize,
        buffer: *mut c_void,
    ) -> Status,
    poll: extern "efiapi" fn(this: &DebugPort) -> Status,
}

impl DebugPort {
    /// Resets the debugport device.
    pub fn reset(&self) -> Result {
        (self.reset)(self).to_result()
    }

    /// Write data to the debugport device.
    ///
    /// Note: `timeout` is given in microseconds
    pub fn write(&self, timeout: u32, data: &[u8]) -> Result<(), usize> {
        let mut buffer_size = data.len();

        (self.write)(
            self,
            timeout,
            &mut buffer_size,
            data.as_ptr().cast::<c_void>(),
        )
        .to_result_with(
            || debug_assert_eq!(buffer_size, data.len()),
            |_| buffer_size,
        )
    }

    /// Read data from the debugport device.
    ///
    /// Note: `timeout` is given in microseconds
    pub fn read(&self, timeout: u32, data: &mut [u8]) -> Result<(), usize> {
        let mut buffer_size = data.len();

        (self.read)(
            self,
            timeout,
            &mut buffer_size,
            data.as_mut_ptr().cast::<c_void>(),
        )
        .to_result_with(
            || debug_assert_eq!(buffer_size, data.len()),
            |_| buffer_size,
        )
    }

    /// Check to see if any data is available to be read from the debugport device.
    pub fn poll(&self) -> Result {
        (self.poll)(self).to_result()
    }
}
