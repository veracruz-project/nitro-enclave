//! RawFD-related material.
//!
//! Definitions for writing and reading buffers to-and-from raw file
//! descriptors.
//!
//! ## Authors
//!
//! The Veracruz Development Team.
//!
//! ## Licensing and copyright notice
//!
//! See the `LICENSE_MIT.markdown` file in the root directory for
//! information on licensing and copyright.

use anyhow::{anyhow, Result};
use byteorder::{ByteOrder, LittleEndian};
use nix::{
    errno::Errno::EINTR,
    sys::socket::{recv, send, MsgFlags},
};
use std::{os::unix::io::RawFd, vec::Vec};

////////////////////////////////////////////////////////////////////////////////
// Sending and receiving data.
////////////////////////////////////////////////////////////////////////////////

/// Send a buffer of data (using a length, buffer protocol) to the file
/// descriptor `fd`
pub fn send_buffer(fd: RawFd, buffer: &[u8]) -> Result<()> {
    let len = buffer.len();
    // first, send the length of the buffer
    {
        let mut buf = [0u8; 9];
        LittleEndian::write_u64(&mut buf, buffer.len() as u64);
        let mut sent_bytes = 0;
        while sent_bytes < buf.len() {
            sent_bytes += send(fd, &buf[sent_bytes..buf.len()], MsgFlags::empty())?;
        }
    }
    // next, send the buffer
    {
        let mut sent_bytes = 0;
        while sent_bytes < len {
            let size = match send(fd, &buffer[sent_bytes..len], MsgFlags::empty()) {
                Ok(size) => size,
                Err(EINTR) => 0,
                Err(err) => {
                    return Err(anyhow!(err));
                }
            };
            sent_bytes += size;
        }
    }
    Ok(())
}

/// Read a buffer of data (using a length, buffer protocol) from the file
/// descriptor `fd`
pub fn receive_buffer(fd: RawFd) -> Result<Vec<u8>> {
    // first, read the length
    let length = {
        let mut buf = [0u8; 9];
        let len = buf.len();
        let mut received_bytes = 0;
        while received_bytes < len {
            received_bytes += match recv(fd, &mut buf[received_bytes..len], MsgFlags::empty()) {
                Ok(size) => size,
                Err(EINTR) => 0,
                Err(err) => {
                    println!("I have experienced an error:{:?}", err);
                    return Err(anyhow!(err));
                }
            }
        }
        LittleEndian::read_u64(&buf) as usize
    };
    let mut buffer: Vec<u8> = vec![0; length];
    // next, read the buffer
    {
        let mut received_bytes: usize = 0;
        while received_bytes < length {
            received_bytes += match recv(fd, &mut buffer[received_bytes..length], MsgFlags::empty())
            {
                Ok(size) => size,
                Err(EINTR) => 0,
                Err(err) => {
                    return Err(anyhow!(err));
                }
            }
        }
    }
    Ok(buffer)
}

