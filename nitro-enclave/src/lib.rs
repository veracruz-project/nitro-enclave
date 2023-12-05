//! Nitro Enclave management code.
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
use err_derive::Error;
use nix::unistd::alarm;
use raw_fd;
use serde_json::Value;
use std::{os::unix::io::AsRawFd, process::Command};

use vsocket;

/// Errors generated by Nitro enclave components of Veracruz
#[derive(Debug, Error)]
pub enum NitroError {
    /// An error occurred while serializing or deserializing
    #[error(display = "Nitro: Serde Error")]
    SerdeError,
}

/// a struct for holding all of the information about a nitro enclave
pub struct NitroEnclave {
    /// The enclave ID, as generated from the Nitro CLI tool when the enclave
    /// is created - it's the EC2-instance ID appended with an enclave-specific
    /// value
    enclave_id: String,
    /// A convenience struct for handling VSOCK connections to the enclave
    vsocksocket: vsocket::VsockSocket,
    /// the path to the Nitro CLI function. Not all AMI images have it in the
    /// same place in the file system, so we need to keep track of it
    nitro_cli_path: String,
}

/// Delay (in seconds) before terminating this process with SIGALRM if
/// the attempt to "connect" to the enclave does not return.
const NITRO_ENCLAVE_CONNECT_TIMEOUT: u32 = 30;

impl NitroEnclave {
    /// create a new Nitro enclave, started with the file in eif_path
    /// * `eif_path` - path to the EIF file that will be started in the enclave
    /// * `debug` - indicates of the enclave will be started in debug mode
    /// * `max_memory_mib` - The amount of memory to be allocated to the enclave
    /// * `port` - The port number that will be used to communicate with the enclave (The code in the EIF should be 
    ///            written to communicate on this port as well)
    pub fn new(eif_path: &str, debug: bool, max_memory_mib: u32, port: u32) -> Result<Self> {
        let max_memory_mib_str = max_memory_mib.to_string();
        let mut args = vec![
            "run-enclave",
            "--eif-path",
            eif_path,
            "--cpu-count",
            "2",
            "--memory",
            &max_memory_mib_str,
        ];
        if debug {
            args.push("--debug-mode=true");
        }
        let nitro_cli_path = "/usr/bin/nitro-cli";
        let stdout = loop {
            let enclave_result = Command::new(nitro_cli_path)
                .args(&args)
                .output()
                .map_err(|err| err);
            match enclave_result {
                Err(err) => {
                    println!("NitroEnclave::new failed to start enclave:{:?}", err);
                    println!("sleeping before trying again");
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    continue;
                }
                Ok(result) => {
                    if !result.status.success() {
                        let enclave_result_stderr = std::str::from_utf8(&result.stderr)?;
                        println!("NitroEnclave::new CLI error:{:?}", enclave_result_stderr);
                        println!("sleeping before trying again");
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                        continue;
                    } else {
                        break result.stdout;
                    }
                }
            }
        };

        let enclave_result_stdout = std::str::from_utf8(&stdout)?;
        println!("enclave_result_stdout:{:?}", enclave_result_stdout);

        let enclave_data: Value = serde_json::from_str(enclave_result_stdout)?;
        let cid: u32 = if !enclave_data["EnclaveCID"].is_number() {
            return Err(anyhow!(NitroError::SerdeError));
        } else {
            serde_json::from_value(enclave_data["EnclaveCID"].clone()).unwrap()
        };

        alarm::set(NITRO_ENCLAVE_CONNECT_TIMEOUT);
        let vsocket = vsocket::VsockSocket::connect(cid, port)?;
        alarm::cancel();

        let enclave: Self = NitroEnclave {
            enclave_id: enclave_data["EnclaveID"]
                .to_string()
                .trim_matches('"')
                .to_string(),
            vsocksocket: vsocket,
            nitro_cli_path: nitro_cli_path.to_string(),
        };
        Ok(enclave)
    }

    /// send a buffer of data to the enclave
    pub fn send_buffer(&self, buffer: &[u8]) -> Result<()> {
        crate::raw_fd::send_buffer(self.vsocksocket.as_raw_fd(), buffer)
    }

    /// receive a buffer of data from the enclave
    pub fn receive_buffer(&self) -> Result<Vec<u8>> {
        crate::raw_fd::receive_buffer(self.vsocksocket.as_raw_fd())
    }
}

impl Drop for NitroEnclave {
    /// Drop the enclave. In ideal conditions, this means that the enclave will
    /// be terminated.
    fn drop(&mut self) {
        // shutdown the enclave
        loop {
            let enclave_result = Command::new(&self.nitro_cli_path)
                .args(&["terminate-enclave", "--enclave-id", &self.enclave_id])
                .output();
            match enclave_result {
                Err(err) => {
                    println!("NitroEnclave::drop Command::new returned err:{:?}, sleeping and trying again", err);
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    continue;
                }
                Ok(result) => {
                    if !result.status.success() {
                        println!("NitroEnclave::drop failed to terminate the enclave (exit_status:{:?}. You will need to terminate it yourself.", result.status);
                        let result_stderr = std::str::from_utf8(&result.stderr).unwrap();
                        println!("NitroEnclave::drop CLI error:{:?}", result_stderr);
                    }
                    break;
                }
            }
        }
    }
}

