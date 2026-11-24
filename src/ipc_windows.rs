use crate::{discord_ipc::DiscordIpc, error::Error};
use serde_json::json;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::PathBuf,
};

type Result<T> = std::result::Result<T, Error>;

#[allow(dead_code)]
#[derive(Debug)]
/// A wrapper struct for the functionality contained in the
/// underlying [`DiscordIpc`](trait@DiscordIpc) trait.
pub struct DiscordIpcClient {
    /// Client ID of the IPC client.
    pub client_id: String,
    socket: Option<File>,
}

impl DiscordIpcClient {
    /// Creates a new `DiscordIpcClient`.
    ///
    /// # Examples
    /// ```
    /// let ipc_client = DiscordIpcClient::new("<some client id>");
    /// ```
    pub fn new<T: AsRef<str>>(client_id: T) -> Self {
        Self {
            client_id: client_id.as_ref().to_string(),
            socket: None,
        }
    }
}

impl DiscordIpc for DiscordIpcClient {
    fn connect_ipc(&mut self) -> Result<()> {
        for i in 0..10 {
            let path = PathBuf::from(format!(r"\\?\pipe\discord-ipc-{}", i));

            match OpenOptions::new().access_mode(0x3).open(&path) {
                Ok(handle) => {
                    self.socket = Some(handle);
                    return Ok(());
                }
                Err(_) => continue,
            }
        }

        Err(Error::IPCConnectionFailed)
    }

    fn write(&mut self, data: &[u8]) -> Result<()> {
        let socket = self.socket.as_mut().ok_or(Error::NotConnected)?;

        socket.write_all(data).map_err(Error::WriteError)?;

        Ok(())
    }

    fn read(&mut self, buffer: &mut [u8]) -> Result<()> {
        let socket = self.socket.as_mut().ok_or(Error::NotConnected)?;

        socket.read_exact(buffer).map_err(Error::ReadError)?;

        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        let data = json!({});
        if self.send(data, 2).is_ok() {}

        let mut socket = self.socket.take().ok_or(Error::NotConnected)?;

        socket.flush().map_err(Error::FlushError)?;
        drop(socket);

        Ok(())
    }

    fn get_client_id(&self) -> &str {
        &self.client_id
    }

    fn try_recv(&mut self) -> Result<Option<(u32, serde_json::Value)>> {
        use std::{
            ffi::c_void,
            os::windows::io::AsRawHandle,
        };

        #[link(name = "kernel32")]
        extern "system" {
            fn PeekNamedPipe(
                pipe: *mut c_void,
                buffer: *mut c_void,
                buffer_size: u32,
                bytes_read: *mut u32,
                total_available: *mut u32,
                bytes_left: *mut u32,
            ) -> i32;
        }

        let socket = self.socket.as_ref().ok_or(Error::NotConnected)?;
        let handle = socket.as_raw_handle() as *mut c_void;

        let mut header = [0u8; 8];
        let mut read = 0u32;
        let mut available = 0u32;

        let ok = unsafe {
            PeekNamedPipe(
                handle,
                header.as_mut_ptr().cast(),
                8,
                &mut read,
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(Error::ReadError(std::io::Error::last_os_error()));
        }
        if available < 8 {
            return Ok(None);
        }

        let (_, length) = unpack(&header)?;
        let frame_len = 8 + length as usize;
        let mut frame = vec![0u8; frame_len];
        let mut read = 0u32;
        let mut available = 0u32;

        let ok = unsafe {
            PeekNamedPipe(
                handle,
                frame.as_mut_ptr().cast(),
                frame_len as u32,
                &mut read,
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(Error::ReadError(std::io::Error::last_os_error()));
        }
        if available < frame_len as u32 {
            return Ok(None);
        }

        self.recv().map(Some)
    }
}
