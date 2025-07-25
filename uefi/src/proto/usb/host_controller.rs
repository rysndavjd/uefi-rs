// SPDX-License-Identifier: MIT OR Apache-2.0

//! USB Host Controller protocol.

//use core::ffi;

use uefi_macros::unsafe_protocol;
use uefi_raw::protocol::usb::host_controller::{Usb2HostControllerProtocol, Speed, 
    ResetAttributes, HostControllerState};
//use uefi_raw::protocol::usb::{
//    ConfigDescriptor, DataDirection, DeviceDescriptor, DeviceRequest, EndpointDescriptor,
//    InterfaceDescriptor, UsbTransferStatus,
//};

//use crate::data_types::PoolString;
use crate::{Result, StatusExt};

/// USB Host Controller protocol.
#[derive(Debug)]
#[repr(transparent)]
#[unsafe_protocol(Usb2HostControllerProtocol::GUID)]
pub struct UsbHostController(Usb2HostControllerProtocol);

impl UsbHostController {
    /// Retrieves the Host Controller capabilities.
    pub fn get_capability(&self) -> Result<(Speed, u8, bool)> {
        let mut speed = unsafe { core::mem::zeroed() };
        let mut port= 0;
        let mut is_64_bit_capable = 0;

        unsafe { (self.0.get_capability)(&self.0, &mut speed, &mut port, &mut is_64_bit_capable) }
            .to_result_with_val(|| (speed, port, is_64_bit_capable == 1))
    }

    /// Software reset for the USB host controller.
    pub fn reset(&mut self, attributes: ResetAttributes) -> Result {
        unsafe { (self.0.reset)(&mut self.0, attributes) }.to_result()
    }

    /// Retrieves current state of the USB host controller.
    pub fn get_state(&mut self) -> Result<HostControllerState> {
        let mut state = unsafe { core::mem::zeroed() };

        unsafe { (self.0.get_state)(&mut self.0, &mut state) }
            .to_result_with_val(|| state)
    }

    /// Sets the USB host controller to a specific state.
    pub fn set_state(&mut self, state: HostControllerState) -> Result {
        unsafe { (self.0.set_state)(&mut self.0, state) }.to_result()
    }

    /* 
    pub fn control_transfer(
        &mut self, 
    ) -> Result<(), UsbTransferStatus> {
        
    }
    */
}